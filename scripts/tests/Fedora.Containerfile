FROM registry.fedoraproject.org/fedora:43
RUN dnf -y install systemd PackageKit polkit shadow-utils passwd rpm-build rpm-sign createrepo_c gnupg2 python3-gobject-base python3-pyte sudo polkit-kde kf6-kirigami qt6-qtquickcontrols2 xorg-x11-server-Xvfb xdotool dpkg dbus-daemon util-linux procps-ng && dnf clean all
CMD ["/sbin/init"]
