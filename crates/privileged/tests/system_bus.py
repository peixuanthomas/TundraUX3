#!/usr/bin/env python3
"""Opt-in negative tests against the installed Linux system-bus service.

No shutdown, reboot, valid update, or generic command is ever requested. The
only valid action is a one-record ReadSystemLogs request, which must be denied
for each supplied negative-test subject. Requires distro python3-dbus.
"""
import argparse
import json
import os
import sys
import uuid
import xml.etree.ElementTree as ET

import dbus

DESTINATION = "org.tundra.Privileged1"
PATH = "/org/tundra/Privileged1"
INTERFACE = DESTINATION
DENIED = "org.freedesktop.DBus.Error.AccessDenied"
INVALID = "org.freedesktop.DBus.Error.InvalidArgs"
UNKNOWN = "org.freedesktop.DBus.Error.UnknownObject"


def run(actor, foreign_operation_id=None):
    if actor == "root" and os.geteuid() != 0:
        raise RuntimeError("root actor requires actual UID 0")
    if actor != "root" and os.geteuid() == 0:
        raise RuntimeError("negative user actors must not run as root")

    bus = dbus.SystemBus(private=True)
    bus_proxy = dbus.Interface(bus.get_object("org.freedesktop.DBus", "/org/freedesktop/DBus"), "org.freedesktop.DBus")
    owner = str(bus_proxy.GetNameOwner(DESTINATION))
    if int(bus_proxy.GetConnectionUnixUser(owner)) != 0:
        raise RuntimeError("refusing tests against a non-root service owner")
    # Pin the exact service instance throughout this run.
    obj = bus.get_object(owner, PATH, introspect=False)
    service = dbus.Interface(obj, INTERFACE)
    results = []

    def record(name, status, **details):
        results.append({"case": name, "status": status, **details})

    def expect_error(name, method, arguments, allowed):
        try:
            value = method(*arguments, timeout=10)
        except dbus.DBusException as error:
            actual = error.get_dbus_name()
            record(name, "passed" if actual in allowed else "failed", error=actual, message=error.get_dbus_message())
        else:
            # Request must not succeed. Try to cancel only that returned ID so
            # a regression cannot leave this harmless log request pending.
            if name.endswith("request_denied") and isinstance(value, str):
                try:
                    service.Cancel(value, timeout=5)
                except dbus.DBusException:
                    pass
            record(name, "failed", unexpected_response=str(value)[:256])

    # Capture actual logind provenance; the SSH test is invalid without Remote.
    logind = dbus.Interface(bus.get_object("org.freedesktop.login1", "/org/freedesktop/login1"), "org.freedesktop.login1.Manager")
    session = None
    try:
        session_path = logind.GetSessionByPID(dbus.UInt32(os.getpid()))
        props = dbus.Interface(bus.get_object("org.freedesktop.login1", session_path), "org.freedesktop.DBus.Properties")
        session = {
            "id": str(props.Get("org.freedesktop.login1.Session", "Id")),
            "uid": int(props.Get("org.freedesktop.login1.Session", "User")[0]),
            "remote": bool(props.Get("org.freedesktop.login1.Session", "Remote")),
            "active": bool(props.Get("org.freedesktop.login1.Session", "Active")),
            "seat": str(props.Get("org.freedesktop.login1.Session", "Seat")[0]),
        }
    except dbus.DBusException:
        pass
    if actor == "ssh" and (not session or not session["remote"] or session["uid"] != os.geteuid()):
        raise RuntimeError("SSH actor must be the real user of a remote logind session")

    version = int(service.ProtocolVersion(timeout=10))
    record("protocol_version", "passed" if version == 1 else "failed", actual=version)
    eligible = bool(service.CanRequest(timeout=10))
    record(actor + "_can_request_false", "passed" if not eligible else "failed", actual=eligible)
    harmless_action = json.dumps({"ReadSystemLogs": {"max_records": 1, "since_epoch_seconds": 0}})
    expect_error(actor + "_request_denied", service.Request, [harmless_action], {DENIED})

    for name, payload in [
        ("malformed_json", "{"),
        ("missing_action", "{}"),
        ("oversized_request", json.dumps("x" * 4097)),
        ("unknown_command_action", json.dumps({"RunCommand": {"argv": ["/usr/bin/true"]}})),
        ("spoofed_uid_parameter", json.dumps({"ReadSystemLogs": {"max_records": 1, "since_epoch_seconds": 0, "uid": 0}})),
        ("spoofed_session_parameter", json.dumps({"ReadSystemLogs": {"max_records": 1, "since_epoch_seconds": 0, "session_id": "1"}})),
        ("zero_record_limit", json.dumps({"ReadSystemLogs": {"max_records": 0, "since_epoch_seconds": 0}})),
        ("excessive_record_limit", json.dumps({"ReadSystemLogs": {"max_records": 10001, "since_epoch_seconds": 0}})),
        ("negative_timestamp", json.dumps({"ReadSystemLogs": {"max_records": 1, "since_epoch_seconds": -1}})),
        ("update_path_traversal", json.dumps({"InstallUpdate": {"release_id": "../../tmp/not-a-release"}})),
    ]:
        expect_error(name, service.Request, [payload], {INVALID})

    unknown_id = uuid.uuid4().hex
    expect_error("unknown_operation_result", service.GetResult, [unknown_id], {UNKNOWN})
    expect_error("unknown_operation_cancel", service.Cancel, [unknown_id], {UNKNOWN})
    if foreign_operation_id:
        # UnknownObject is explicitly NOT success: this requires an existing
        # operation belonging to another still-live unique bus sender.
        expect_error("foreign_sender_result", service.GetResult, [foreign_operation_id], {DENIED})
        expect_error("foreign_sender_cancel", service.Cancel, [foreign_operation_id], {DENIED})
    else:
        record("foreign_sender_result", "skipped", reason="No existing foreign operation supplied; unknown-ID denial is not ownership proof")

    introspection = dbus.Interface(obj, "org.freedesktop.DBus.Introspectable").Introspect(timeout=10)
    root = ET.fromstring(str(introspection))
    methods = {method.attrib["name"] for interface in root.findall("interface") if interface.attrib["name"] == INTERFACE for method in interface.findall("method")}
    expected = {"ProtocolVersion", "CanRequest", "Request", "GetResult", "Cancel"}
    record("bounded_method_surface", "passed" if methods == expected else "failed", methods=sorted(methods))
    expect_error("no_generic_command_method", service.RunCommand, ["/usr/bin/true"], {"org.freedesktop.DBus.Error.UnknownMethod"})

    if actor != "root":
        expect_error("nonroot_cannot_own_service_name", bus_proxy.RequestName, [DESTINATION, dbus.UInt32(4)], {DENIED})
    else:
        record("nonroot_cannot_own_service_name", "skipped", reason="root owns system services by design; covered by user actors")

    record("same_service_instance", "passed" if str(bus_proxy.GetNameOwner(DESTINATION)) == owner else "failed")
    report = {
        "actor": actor,
        "uid": os.getuid(),
        "euid": os.geteuid(),
        "groups": os.getgroups(),
        "sender": bus.get_unique_name(),
        "service_owner": owner,
        "logind_session": session,
        "cases": results,
    }
    print(json.dumps(report, ensure_ascii=False, indent=2))
    bus.close()
    return 1 if any(case["status"] == "failed" for case in results) else 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--actor", choices=("root", "ssh", "ordinary"), required=True)
    parser.add_argument("--foreign-operation-id")
    args = parser.parse_args()
    try:
        sys.exit(run(args.actor, args.foreign_operation_id))
    except Exception as error:
        print(json.dumps({"harness_error": type(error).__name__, "message": str(error)}), file=sys.stderr)
        sys.exit(2)
