FROM registry.fedoraproject.org/fedora:43
RUN dnf -y install systemd PackageKit polkit shadow-utils passwd rpm-build rpm-sign createrepo_c gnupg2 python3-gobject-base dbus-daemon util-linux procps-ng && dnf clean all
CMD ["/sbin/init"]
