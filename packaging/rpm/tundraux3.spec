# Binaries are compiled with Cargo.lock by scripts/package-linux.sh.
%global debug_package %{nil}

Name:           tundraux3
Version:        %{tundra_version}
Release:        1
Summary:        Terminal desktop environment experiment
License:        MIT AND GPL-3.0-or-later
URL:            https://github.com/peixuanthomas/TundraUX3
Source0:        %{name}-%{version}-linux-x86_64.tar.gz
Source1:        tundraux3.desktop
Source2:        tundraux3.pam
ExclusiveArch:  x86_64
Requires:       xdg-utils
Requires:       glib2
Requires:       pam
Requires:       glibc
Requires:       sudo
Recommends:     dbus
Recommends:     xdg-desktop-portal
Recommends:     polkit
Recommends:     xorg-x11-server-Xwayland

%description
TundraUX3 provides a full-screen terminal shell and its management CLI.
This package includes the default assets and Fedora system PAM integration.

%prep
%setup -q -n %{name}-%{version}-linux-x86_64

%build
# The portable archive contains the locked Cargo release build.

%install
install -Dm755 tundra-shell %{buildroot}%{_bindir}/tundra-shell
install -Dm755 tundra-cli %{buildroot}%{_bindir}/tundra-cli
install -d %{buildroot}%{_datadir}/%{name}
cp -a assets %{buildroot}%{_datadir}/%{name}/assets
install -Dm644 %{SOURCE1} %{buildroot}%{_datadir}/applications/tundraux3.desktop
install -Dm644 %{SOURCE2} %{buildroot}%{_sysconfdir}/pam.d/tundraux3

%files
%license LICENSE LICENSE.weathr
%doc README-LINUX.txt
%{_bindir}/tundra-shell
%{_bindir}/tundra-cli
%{_datadir}/%{name}/
%{_datadir}/applications/tundraux3.desktop
%config(noreplace) %{_sysconfdir}/pam.d/tundraux3
