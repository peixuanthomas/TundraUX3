# Binaries are built with Cargo.lock by scripts/package-linux.sh.
%global debug_package %{nil}
%global _build_id_links none
# Payload hashes bind prebuilt executables/modules. RPM brp stripping or note
# rewriting after the capability record is emitted would invalidate sessiond
# verification. The trusted build produces final bytes before staging.
%global __os_install_post %{nil}

Name:           tundraux3
Version:        %{tundra_version}
Release:        1
Summary:        User desktop with opt-in PAM sessions and protected system services
License:        MIT AND GPL-3.0-or-later
URL:            https://github.com/peixuanthomas/TundraUX3
Source0:        %{name}-%{version}-linux-x86_64.tar.gz
ExclusiveArch:  x86_64
Requires:       xdg-utils
Requires:       glib2
Requires:       pam
Requires:       glibc
Requires:       systemd
Requires:       dbus
Requires:       curl
Requires:       gh >= 2.87.3
Requires:       libseat
Requires:       libdrm
Requires:       pango
Provides:       bundled(libtsm) = 4.7.0
Provides:       bundled(kmscon) = 10.0.3
Requires:       font(notosansmonocjksc)
Requires(post): systemd
Requires:       policycoreutils
Requires:       selinux-policy-targeted
Recommends:     xdg-desktop-portal

%description
TundraUX3 runs the desktop as an ordinary user. This package includes optional
PAM/logind session and authorization services. Installation never starts the seat,
enables desktop services, or replaces the existing display manager.

%prep
%setup -q -n %{name}-%{version}-linux-x86_64

%build
# The archive contains the common, already validated system-root staging tree.

%install
mkdir -p %{buildroot}
cp -a system-root/. %{buildroot}/

%post
set -eu
systemd-sysusers /usr/lib/sysusers.d/tundra.conf
systemd-tmpfiles --create /usr/lib/tmpfiles.d/tundra.conf
release=$(cat /usr/share/tundra/bootstrap-release)
case "$release" in v[0-9]*.[0-9]*.[0-9]*) ;; *) exit 1;; esac
current=/var/lib/tundra/runtime/current
if [ ! -e "$current" ] && [ ! -L "$current" ]; then
  ln -s "versions/$release" "$current"
elif [ -L "$current" ] && [ ! -e "$current" ]; then
  # The previous OS package's bootstrap version may have been removed.
  # Preserve every still-existing online version; repair only a dangling
  # version pointer using the newly installed trusted bootstrap.
  case "$(readlink "$current")" in versions/v*) ;; *) exit 1;; esac
  pending="/var/lib/tundra/runtime/bootstrap-current.$$"
  ln -s "versions/$release" "$pending"
  mv -T "$pending" "$current"
fi
if [ -e /sys/fs/selinux/enforce ]; then
  semodule -X 100 -i /usr/share/selinux/packages/tundra-runtime.cil
  restorecon -RF /var/lib/tundra/runtime/versions
else
  # Image builds prepare the persistent policy store without a running kernel.
  semodule -n -X 100 -i /usr/share/selinux/packages/tundra-runtime.cil
fi
if [ -d /run/systemd/system ]; then systemctl daemon-reload; fi
if [ -S /run/dbus/system_bus_socket ]; then
  busctl --system call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig
fi

%postun
if [ -d /run/systemd/system ]; then systemctl daemon-reload; fi
if [ -S /run/dbus/system_bus_socket ]; then
  busctl --system call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig
fi

%files
%defattr(-,root,root,-)
%{_bindir}/tundra-shell
%{_bindir}/tundra-cli
/usr/libexec/tundra/
/usr/lib/systemd/system/tundra-*.service
/usr/lib/sysusers.d/tundra.conf
/usr/lib/tmpfiles.d/tundra.conf
%{_datadir}/dbus-1/system.d/org.tundra.*.conf
%{_datadir}/tundra/
%{_datadir}/tundraux3/
%{_datadir}/selinux/packages/tundra-runtime.cil
%{_datadir}/applications/tundraux3.desktop
%{_datadir}/doc/tundraux3/
%dir /var/lib/tundra
%dir /var/lib/tundra/runtime
%dir /var/lib/tundra/runtime/versions
/var/lib/tundra/runtime/versions/v%{version}/
%dir %{_sysconfdir}/tundra
%config(noreplace) %{_sysconfdir}/pam.d/tundra-session
%config(noreplace) %{_sysconfdir}/pam.d/tundra-greeter
%config(noreplace) %{_sysconfdir}/tundra/privileged.toml
%{_sysconfdir}/tundra/update-trusted-root.jsonl
