#!/usr/bin/env bash
set -euo pipefail
# Build the private terminal from pinned sources. Never install onto the host.
[[ "$(uname -s)" == Linux ]] || { echo 'kmscon build requires Linux' >&2; exit 1; }
[[ $# -eq 2 && "$1" == --output ]] || { echo "Usage: $0 --output EMPTY_DIRECTORY" >&2; exit 2; }
for tool in git meson ninja pkg-config readelf python3; do command -v "$tool" >/dev/null; done
for dependency in libseat libdrm libudev xkbcommon pangoft2 zlib; do pkg-config --exists "$dependency"; done
output="$2"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
[[ -z "$(ls -A "$output")" ]] || { echo 'kmscon output directory must be empty' >&2; exit 1; }
work="$(mktemp -d "${TMPDIR:-/tmp}/tundra-kmscon-build.XXXXXXXX")"
trap 'rm -rf -- "$work"' EXIT
kmscon_commit=ad9c77bc04f718d0f0d6dfc51291b7d652336429
libtsm_commit=facadce47638f52dfc8939b463e98f0962863aad
fetch_pinned() {
  local source_url="$1" source_commit="$2" destination="$3"
  git init -q "$destination"
  git -C "$destination" fetch -q --depth=1 "$source_url" "$source_commit"
  git -C "$destination" -c advice.detachedHead=false checkout -q --detach FETCH_HEAD
  [[ "$(git -C "$destination" rev-parse HEAD)" == "$source_commit" ]]
}
fetch_pinned https://github.com/kmscon/kmscon.git "$kmscon_commit" "$work/source"
fetch_pinned https://github.com/kmscon/libtsm.git "$libtsm_commit" "$work/source/subprojects/libtsm"
# Upstream hardcodes shared_library, which defeats default_library=static. This
# exact build-description patch allows the pinned tsm to be embedded on distros
# whose system tsm ABI is too old.
python3 - "$work/source/subprojects/libtsm/src/tsm/meson.build" <<'PY'
from pathlib import Path
import sys
path=Path(sys.argv[1]); text=path.read_text()
old='libtsm = shared_library('
if text.count(old) != 1:
    raise SystemExit('Unexpected pinned libtsm build description')
path.write_text(text.replace(old,'libtsm = library(',1))
PY
# libseat/logind supplies an already-master DRM descriptor to the ordinary UID.
# Calling drmSetMaster again requires privilege and fails with EPERM, even though
# this descriptor already has exactly the authority the renderer needs.
python3 - "$work/source/src/video/drm_shared.c" <<'PY_DRM'
from pathlib import Path
import sys
path=Path(sys.argv[1]); text=path.read_text()
old='\tret = drmSetMaster(vdrm->fd);'
if text.count(old) != 1:
    raise SystemExit('Unexpected pinned kmscon DRM master implementation')
replacement='\tif (drmIsMaster(vdrm->fd)) {\n\t\tvdrm->master = true;\n\t\treturn 0;\n\t}\n\n'+old
path.write_text(text.replace(old,replacement,1))
PY_DRM
# logind revocation may deliver HUP/ENODEV before libseat's disable callback.
# Retain brokered input nodes for resume (udev removal still frees real removals)
# and acknowledge the completed pause so the broker can activate the other seat.
python3 - "$work/source" <<'PY_INPUT'
from pathlib import Path
import sys
root=Path(sys.argv[1])
def replace_once(path, old, new):
    text=path.read_text()
    if text.count(old) != 1:
        raise SystemExit('Unexpected pinned libseat input implementation: '+str(path))
    path.write_text(text.replace(old,new,1))
path=root/'src/input/input.c'
replace_once(path, 'static void input_free_dev(struct input_dev *dev);',
             'static void input_free_dev(struct input_dev *dev);\nstatic void input_sleep_dev(struct input_dev *dev);')
replace_once(path, 'log_debug("EOF on %s", dev->node);\n\t\tinput_free_dev(dev);',
             'log_debug("EOF on %s", dev->node);\n\t\tif (dev->input->open_cb)\n\t\t\tinput_sleep_dev(dev);\n\t\telse\n\t\t\tinput_free_dev(dev);')
replace_once(path, 'log_warn("reading from %s failed (%d): %m", dev->node, errno);\n\t\t\tinput_free_dev(dev);',
             'log_warn("reading from %s failed (%d): %m", dev->node, errno);\n\t\t\tif (dev->input->open_cb && errno == ENODEV)\n\t\t\t\tinput_sleep_dev(dev);\n\t\t\telse\n\t\t\t\tinput_free_dev(dev);\n\t\t\treturn;')
path=root/'src/uterm/vt_libseat.c'
old='\tvt_cb_deactivate(&vt->base, false);\n\ttty_deactivate(vt);'
replace_once(path, old, old+'\n\tlibseat_disable_seat(libseat);')
PY_INPUT
export SOURCE_DATE_EPOCH
SOURCE_DATE_EPOCH="$(git -C "$work/source" show -s --format=%ct HEAD)"
meson setup "$work/build" "$work/source" --buildtype=release \
  --prefix=/usr --libdir=/usr/libexec/tundra/modules --sysconfdir=/etc/tundra \
  --wrap-mode=nodownload --force-fallback-for=libtsm \
  -Dlibseat=enabled -Dfont_pango=enabled -Dfont_freetype=disabled \
  -Dfont_unifont=disabled -Dfont_psf=disabled \
  -Dvideo_drm2d=enabled -Dvideo_drm3d=disabled -Dvideo_fbdev=disabled \
  -Drenderer_gltex=disabled -Ddbus=disabled -Dtests=false -Ddocs=disabled \
  -Dlibtsm:default_library=static -Dlibtsm:tests=false -Dlibtsm:gtktsm=false
meson compile -C "$work/build" -j "${TUNDRAUX3_BUILD_JOBS:-2}"
program="$work/build/src/kmscon"
module="$work/build/src/font/mod-pango.so"
# --libseat appearing in --help is not proof: distro builds accept that option
# even when the backend is omitted. Require actual dynamic linkage to libseat.
readelf -d "$program" | grep -E 'NEEDED.*libseat\.so\.' >/dev/null
if readelf -d "$program" "$module" | grep -E 'NEEDED.*libtsm\.so\.' >/dev/null; then
  echo 'Pinned libtsm must be embedded; refusing a distro ABI dependency' >&2
  exit 1
fi
install -m755 "$program" "$output/kmscon"
install -m644 "$module" "$output/mod-pango.so"
install -m644 "$work/source/COPYING" "$output/LICENSE.kmscon"
install -m644 "$work/source/subprojects/libtsm/COPYING" "$output/LICENSE.libtsm"
python3 - "$output" "$kmscon_commit" <<'PY'
import hashlib,json,sys
from pathlib import Path
root=Path(sys.argv[1])
capabilities=dict(sha256=hashlib.sha256((root/'kmscon').read_bytes()).hexdigest(),
                  pango_sha256=hashlib.sha256((root/'mod-pango.so').read_bytes()).hexdigest(),
                  libseat=True,source_commit=sys.argv[2])
(root/'kmscon-capabilities.json').write_text(json.dumps(capabilities,sort_keys=True)+'\n')
PY
chmod 644 "$output/kmscon-capabilities.json"
printf 'Built pinned libseat-enabled kmscon at %s\n' "$output"
