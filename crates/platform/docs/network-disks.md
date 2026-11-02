# Network and disk operations

## Network connections and recovery

The Network page lists interfaces and Wi-Fi networks discovered by NetworkManager, showing name, signal, encryption, saved state, and connection state. Open networks connect directly; encrypted networks ask for a password in a form. Hidden networks and other password entry points are under More Actions.

Before connecting, an independent recovery service is prepared in the background, then the network is switched. The helper checks the connection UUID, network name, and address acquisition result, and keeps the connection on success. An incorrect password, timeout, or cancellation restores the original connection. Passwords travel through D-Bus and are not written to command lines or ordinary logs.

Address, gateway, and DNS changes use a 120-second confirmation window to keep the changes. Scripts can set `defer_confirmation=true`, receive a task ID, then query the result and confirm; the independent recovery service handles timeouts.

Network Diagnostics checks the device, wireless switch, connection, address, routes, DNS, and target port in order. Results offer actions to check again, select a network, or open configuration.

Network changes and saving or forgetting Wi-Fi connections are recorded in the shared configuration backup system. Before restoring a configuration in use, check file versions, device, MAC, native connection, and the currently active connection, then apply it temporarily and require a new confirmation within 120 seconds. Restoring a saved Wi-Fi configuration that is not in use only reloads configuration without switching the current network.

## Disks, mounts, and health

The Disks page shows total and remaining byte capacity and inode counts, along with usage percentages. File quotas not reported by the system are shown as unknown.

Mount at Boot generates an `/etc/fstab` draft and opens it in the editor:

- Identify filesystems by UUID, preserving other entries and comments.
- Existing device-path entries can be converted to UUIDs.
- Duplicate UUIDs, occupied mount points, or devices that cannot be confirmed report a specific reason.
- Check the original content on which the draft was based before saving; saving does not mount immediately.

Check Disk Health first reads UDisks SMART and uses `smartctl` JSON when valid data is unavailable. Queries do not start self-tests, change disk settings, or deliberately wake standby disks. Temperature, bad sectors, and sample time appear separately; missing fields are shown as unknown.

## Directory scans

Directory scans use the original user's permissions, skip symbolic links, other mounts, and bind mounts, and count hard links only once.

Results can be sorted by allocated bytes, content bytes, or file count without rescanning. Directory rows open File Explorer; file rows open their containing directory and select the file. Device, file ID, and type can be checked before opening; results are retained on return.

## Validation and references

Backend unit tests and read-only interfaces can run on Linux/WSL. Wi-Fi association, recovery from incorrect passwords, independent recovery after disconnection, and physical-disk SMART are verified on the corresponding Linux host.

References: [NetworkManager access points](https://www.networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.AccessPoint.html), [NetworkManager types](https://www.networkmanager.dev/docs/api/latest/nm-dbus-types.html), [UDisks SMART](https://storaged.org/doc/udisks2-api/latest/gdbus-org.freedesktop.UDisks2.Drive.Ata.html), [fstab](https://man7.org/linux/man-pages/man5/fstab.5.html).
