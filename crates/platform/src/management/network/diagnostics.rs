use super::*;

fn step(id: &str, label: &str, state: &str, detail: String, next: &str) -> ManagementRow {
    ManagementRow {
        id: id.into(),
        cells: vec![label.into(), state.into(), next.into()],
        detail: vec![("Result".into(), detail)],
        ..Default::default()
    }
}

pub(super) fn address_present(addresses: &Json) -> bool {
    addresses
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|device| device["addr_info"].as_array().into_iter().flatten())
        .any(|address| {
            address["scope"].as_str() == Some("global")
                && address["local"]
                    .as_str()
                    .and_then(|s| s.parse::<IpAddr>().ok())
                    .is_some_and(|ip| !ip.is_loopback() && !ip.is_unspecified())
                && !address["flags"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|flag| matches!(flag.as_str(), Some("tentative" | "dadfailed")))
        })
}

pub(super) fn diagnose(
    name: &str,
    values: &BTreeMap<String, String>,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    let host = values
        .get("host")
        .map(String::as_str)
        .unwrap_or("example.com");
    if host.is_empty()
        || host.starts_with('-')
        || host.len() > 253
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
    {
        return Err(ManagementError::InvalidInput(
            "Enter a host name or IP address.".into(),
        ));
    }
    let port = values
        .get("port")
        .map(String::as_str)
        .unwrap_or("443")
        .parse::<u16>()
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| ManagementError::InvalidInput("Enter a port from 1 to 65535.".into()))?;
    check_cancelled(cancelled)?;
    let mut snapshot = ManagementSnapshot {
        columns: vec!["Check".into(), "Result".into(), "Next step".into()],
        backend: format!("Network diagnosis: {name}"),
        ..Default::default()
    };
    let root = PathBuf::from(format!("/sys/class/net/{name}"));
    snapshot.rows.push(step(
        "device",
        "Device",
        if root.exists() { "Passed" } else { "Failed" },
        name.into(),
        if root.exists() { "" } else { "Refresh devices" },
    ));
    let wireless = root.join("wireless").exists();
    let radio = if wireless {
        let bus = bus();
        bus.and_then(|bus| {
            let nm = Proxy::new(&bus, NM, NM_PATH, NM).map_err(dbus_error)?;
            Ok(nm
                .get_property::<bool>("WirelessEnabled")
                .map_err(dbus_error)?
                && nm
                    .get_property::<bool>("WirelessHardwareEnabled")
                    .map_err(dbus_error)?)
        })
    } else {
        Ok(true)
    };
    snapshot.rows.push(match radio {
        Ok(enabled) => step(
            "radio",
            "Wireless switch",
            if !wireless {
                "Not applicable"
            } else if enabled {
                "Passed"
            } else {
                "Failed"
            },
            if enabled {
                "Wireless enabled"
            } else {
                "Wireless disabled or hardware blocked"
            }
            .into(),
            if enabled {
                ""
            } else {
                "Enable Wi-Fi or check the hardware switch"
            },
        ),
        Err(e) => step(
            "radio",
            "Wireless switch",
            "Unknown",
            e.to_string(),
            "Open system network settings",
        ),
    });
    let state =
        std::fs::read_to_string(root.join("operstate")).unwrap_or_else(|_| "unknown".into());
    let connected = if wireless {
        nm_device(name).map(|d| !d.uuid.is_empty()).ok()
    } else {
        match state.trim() {
            "up" => Some(true),
            "down" | "notpresent" | "lowerlayerdown" => Some(false),
            _ => None,
        }
    };
    snapshot.rows.push(step(
        "connection",
        "Connection",
        match connected {
            Some(true) => "Passed",
            Some(false) => "Failed",
            None => "Unknown",
        },
        state.trim().into(),
        if connected == Some(true) {
            ""
        } else if wireless {
            "Choose a Wi-Fi network"
        } else {
            "Check cable and interface state"
        },
    ));
    let addresses =
        run("ip", &["-j", "address", "show", "dev", name], cancelled).and_then(|text| {
            serde_json::from_str::<Json>(&text).map_err(|e| ManagementError::Failed(e.to_string()))
        });
    snapshot.rows.push(match addresses {
        Ok(addresses) => {
            let present = address_present(&addresses);
            step(
                "address",
                "IP address",
                if present { "Passed" } else { "Failed" },
                addresses.to_string(),
                if present {
                    ""
                } else {
                    "Check automatic addressing or configure an address"
                },
            )
        }
        Err(e) => step(
            "address",
            "IP address",
            "Unknown",
            e.to_string(),
            "Refresh interface details",
        ),
    });
    let query_routes = |family: &str| {
        run(
            "ip",
            &["-j", family, "route", "show", "dev", name],
            cancelled,
        )
        .and_then(|text| {
            serde_json::from_str::<Json>(&text).map_err(|e| ManagementError::Failed(e.to_string()))
        })
    };
    let routes = combined_routes(query_routes("-4"), query_routes("-6"));
    snapshot.rows.push(match routes {
        Ok(routes) => {
            let present = routes.as_array().is_some_and(|r| !r.is_empty());
            step(
                "route",
                "Routes",
                if present { "Passed" } else { "Failed" },
                routes.to_string(),
                if present {
                    ""
                } else {
                    "Check gateway and routes"
                },
            )
        }
        Err(e) => step(
            "route",
            "Routes",
            "Unknown",
            e.to_string(),
            "Check gateway and routes",
        ),
    });
    check_cancelled(cancelled)?;
    let ips = if let Ok(ip) = host.parse::<IpAddr>() {
        Ok(vec![ip])
    } else {
        run("getent", &["ahosts", host], cancelled).map(|text| {
            let mut ips = text
                .lines()
                .filter_map(|line| line.split_whitespace().next()?.parse::<IpAddr>().ok())
                .collect::<Vec<_>>();
            ips.sort();
            ips.dedup();
            ips
        })
    };
    let ips = match ips {
        Ok(ips) if !ips.is_empty() => {
            snapshot.rows.push(step(
                "dns",
                "DNS",
                if host.parse::<IpAddr>().is_ok() {
                    "Not applicable"
                } else {
                    "Passed"
                },
                format!("{host}: {ips:?}"),
                "",
            ));
            ips
        }
        Ok(_) => {
            snapshot.rows.push(step(
                "dns",
                "DNS",
                "Failed",
                "No address was returned".into(),
                "Check DNS servers or enter an IP address",
            ));
            Vec::new()
        }
        Err(e) => {
            snapshot.rows.push(step(
                "dns",
                "DNS",
                "Failed",
                e.to_string(),
                "Check DNS servers or enter an IP address",
            ));
            Vec::new()
        }
    };
    let mut errors = Vec::new();
    let mut reachable = false;
    for ip in ips.iter().take(4) {
        check_cancelled(cancelled)?;
        let route =
            run("ip", &["-j", "route", "get", &ip.to_string()], cancelled).and_then(|text| {
                serde_json::from_str::<Json>(&text)
                    .map_err(|e| ManagementError::Failed(e.to_string()))
            });
        match route {
            Ok(route) if route[0]["dev"].as_str() == Some(name) => {}
            Ok(route) => {
                errors.push(format!(
                    "{ip}: route uses {}",
                    route[0]["dev"].as_str().unwrap_or("an unknown interface")
                ));
                continue;
            }
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        }
        match TcpStream::connect_timeout(
            &std::net::SocketAddr::new(*ip, port),
            Duration::from_secs(3),
        ) {
            Ok(_) => {
                reachable = true;
                break;
            }
            Err(e) => errors.push(format!("{ip}: {e}")),
        }
    }
    snapshot.rows.push(step(
        "target",
        "Target connection",
        if ips.is_empty() {
            "Not checked"
        } else if reachable {
            "Passed"
        } else {
            "Failed"
        },
        format!("{host}:{port}\n{}", errors.join("\n")),
        if reachable {
            ""
        } else {
            "Check target, firewall and route; try another target"
        },
    ));
    let failed = snapshot
        .rows
        .iter()
        .filter(|row| row.cells[1] == "Failed")
        .count();
    snapshot.notices.push(if failed == 0 {
        "Checks finished. Target reachability is separate from Wi-Fi connection status.".into()
    } else {
        format!("{failed} checks failed. Follow the next step beside each result.")
    });
    let owner = config::detect(name);
    let mut identity = interface_identity(name)?;
    identity.insert("interface".into(), name.into());
    identity.insert("owner".into(), owner.label().into());
    identity.insert("source".into(), owner.source_display());
    identity.insert("active_uuid".into(), owner.uuid.clone());
    let readable = owner
        .path
        .as_ref()
        .and_then(|path| std::fs::read(path).ok());
    if let Some(bytes) = &readable {
        identity.insert("configuration".into(), fingerprint(bytes));
    }
    for row in &mut snapshot.rows {
        row.identity = identity.clone();
        row.actions.push(ManagementAction {
            id: "check-again".into(),
            label: "Check again".into(),
            primary: true,
            fields: vec![
                field("host", "Host or IP", host, true, &[]),
                field("port", "TCP port", &port.to_string(), true, &[]),
            ],
            ..Default::default()
        });
        if row.cells[1] == "Failed" || row.cells[1] == "Unknown" {
            if matches!(row.id.as_str(), "address" | "route" | "dns") {
                if owner.path.is_some() && readable.is_none() {
                    row.actions.push(ManagementAction {
                        id: "inspect_network".into(),
                        label: "Authorize network inspection".into(),
                        privileged: true,
                        ..Default::default()
                    });
                } else {
                    let mut fields = settings_fields();
                    owner.populate_fields(&mut fields);
                    row.actions.push(ManagementAction {
                        id: "configure".into(),
                        label: "Configure addresses and DNS".into(),
                        fields,
                        privileged: true,
                        confirm: true,
                        disabled_reason: owner.reason.clone().or_else(recovery_unavailable),
                        ..Default::default()
                    });
                }
            }
            row.actions.push(ManagementAction {
                id: "network-list".into(),
                label: "Choose network".into(),
                ..Default::default()
            });
        }
    }
    Ok(snapshot)
}

fn combined_routes(
    ipv4: Result<Json, ManagementError>,
    ipv6: Result<Json, ManagementError>,
) -> Result<Json, ManagementError> {
    let mut rows = Vec::new();
    let mut error = None;
    for family in [ipv4, ipv6] {
        match family {
            Ok(Json::Array(routes)) => rows.extend(routes),
            Ok(_) => {
                error = Some(ManagementError::Failed(
                    "The route query returned an unexpected result".into(),
                ))
            }
            Err(e) => error = Some(e),
        }
    }
    // A known route is enough to pass. If no route was found, failed queries
    // make the result unknown rather than incorrectly reporting no routes.
    if rows.is_empty()
        && let Some(error) = error
    {
        return Err(error);
    }
    Ok(Json::Array(rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tentative_and_link_local_addresses_do_not_pass_acquisition_check() {
        for text in [
            r#"[{"addr_info":[{"scope":"link","local":"fe80::1"}]}]"#,
            r#"[{"addr_info":[{"scope":"global","local":"192.0.2.1","flags":["tentative"]}]}]"#,
        ] {
            assert!(!address_present(&serde_json::from_str(text).unwrap()));
        }
        assert!(address_present(
            &serde_json::json!([{"addr_info":[{"scope":"global","local":"192.0.2.1"}]}])
        ));
    }
    #[test]
    fn ipv6_only_routes_pass_and_incomplete_empty_queries_remain_unknown() {
        assert_eq!(
            combined_routes(
                Ok(serde_json::json!([])),
                Ok(serde_json::json!([{"dst":"default","dev":"eth0"}]))
            )
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
            1
        );
        assert!(
            combined_routes(
                Err(ManagementError::Unavailable("ip missing".into())),
                Ok(serde_json::json!([]))
            )
            .is_err()
        );
        assert!(
            combined_routes(Ok(serde_json::json!([])), Ok(serde_json::json!([])))
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
