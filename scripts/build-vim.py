#!/usr/bin/env python3
"""Build the pinned, unmodified Vim executable and runtime beside Tundra."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile


REPO = Path(__file__).resolve().parent.parent
PIN = json.loads((REPO / "packaging/vim.json").read_text(encoding="utf-8"))
WINDOWS_ICONV_FILES = (
    "libiconv-2.dll", "licenses/libiconv/COPYING.LIB",
    "licenses/libiconv/BUILD-LICENSE.txt", "sources/libiconv.tar.gz",
)


def run(arguments, cwd, env=None):
    subprocess.run(arguments, cwd=cwd, env=env, check=True)


def checked_archive(url, checksum, archive, offline=False):
    if not archive.exists():
        if offline:
            raise RuntimeError(f"Archive does not exist: {archive}")
        partial = archive.with_suffix(".download")
        with urllib.request.urlopen(url, timeout=60) as response, partial.open("wb") as file:
            shutil.copyfileobj(response, file)
        partial.replace(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != checksum:
        raise RuntimeError(f"Archive SHA-256 mismatch: {archive}")
    return archive


def stage_windows_iconv(directory, cache, archive=None, source_archive=None):
    pin = PIN["windows_iconv"]
    binary = checked_archive(
        pin["binary_url"], pin["binary_sha256"],
        archive or cache / pin["binary_url"].rsplit("/", 1)[1], bool(archive))
    source = checked_archive(
        pin["source_url"], pin["source_sha256"],
        source_archive or cache / f"libiconv-{pin['version']}.tar.gz", bool(source_archive))
    licenses = directory / "licenses/libiconv"
    licenses.mkdir(parents=True)
    with zipfile.ZipFile(binary) as bundle:
        (directory / "libiconv-2.dll").write_bytes(bundle.read("bin/libiconv-2.dll"))
        (licenses / "BUILD-LICENSE.txt").write_bytes(bundle.read("license.txt"))
    with tarfile.open(source, "r:gz") as tar:
        license_file = tar.extractfile(f"libiconv-{pin['version']}/COPYING.LIB")
        (licenses / "COPYING.LIB").write_bytes(license_file.read())
    (directory / "sources").mkdir()
    shutil.copy2(source, directory / "sources/libiconv.tar.gz")


def verify_bundle(directory):
    executable = directory / ("vim.exe" if os.name == "nt" else "vim")
    output = subprocess.check_output([str(executable), "--version"], text=True)
    for feature in PIN["required_features"]:
        pattern = r"\+" + re.escape(feature)
        if feature == "multi_byte":
            pattern += r"(?:_ime(?:/dyn)?)?"
        if not re.search(pattern + r"(?:\s|$)", output):
            raise RuntimeError(f"Bundled Vim requires +{feature}")
    major, minor, patch = PIN["version"].split(".")
    if not output.startswith(f"VIM - Vi IMproved {major}.{minor} ") or not re.search(
        r"^Included patches: 1-" + re.escape(patch) + r"$", output, re.MULTILINE
    ):
        raise RuntimeError(f"Bundled Vim must be version {PIN['version']}")
    required = ("runtime/defaults.vim", "runtime/doc/help.txt", "LICENSE", "UPSTREAM.json")
    if os.name == "nt":
        required += WINDOWS_ICONV_FILES
    for relative in required:
        path = directory / relative
        if not path.is_file() or path.stat().st_size == 0:
            raise RuntimeError(f"Incomplete Vim bundle: missing {relative}")
    if json.loads((directory / "UPSTREAM.json").read_text(encoding="utf-8")) != PIN:
        raise RuntimeError("Bundled Vim does not match packaging/vim.json")
    if os.name == "nt":
        run([
            str(executable), "-u", "NONE", "-U", "NONE", "-i", "NONE", "-n", "-es",
            "-c", "call assert_equal([65, 0], blob2list(str2blob(['A'], {'encoding': 'utf-16le'})))",
            "-c", "if !empty(v:errors) | cquit | endif", "-c", "qall!",
        ], directory, dict(os.environ, VIMRUNTIME=str(directory / "runtime")))


def build_windows(source):
    arguments = [
        "nmake", "/nologo", "-f", "Make_mvc.mak", "FEATURES=HUGE", "GUI=no",
        "VIMDLL=no", "CHANNEL=yes", "TERMINAL=yes", "ICONV=yes", "GETTEXT=no", "SOUND=no",
    ]
    if shutil.which("nmake"):
        run(arguments, source / "src")
        return
    vswhere = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / (
        "Microsoft Visual Studio/Installer/vswhere.exe"
    )
    installation = subprocess.check_output([
        str(vswhere), "-latest", "-products", "*", "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath",
    ], text=True).strip()
    if not installation:
        raise RuntimeError("Building Vim on Windows requires Visual Studio C++ build tools")
    developer = Path(installation) / "Common7/Tools/VsDevCmd.bat"
    batch = source / "build-tundra-vim.cmd"
    batch.write_text(
        f'@echo off\ncall "{developer}" -no_logo -arch=x64\n'
        'if errorlevel 1 exit /b %errorlevel%\n'
        + subprocess.list2cmdline(arguments) + '\nexit /b %errorlevel%\n',
        encoding="utf-8",
    )
    run(["cmd.exe", "/d", "/c", str(batch)], source / "src")


def package_bundle(directory, archive):
    if archive.resolve().is_relative_to(directory.resolve()):
        raise ValueError("Engine archive must be outside the Vim directory")
    archive.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        for path in sorted(directory.rglob("*")):
            if path.is_file():
                bundle.write(path, Path("vim") / path.relative_to(directory))
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_suffix(archive.suffix + ".sha256").write_text(
        f"{checksum}  {archive.name}\n", encoding="utf-8")
    print(f"Packaged Vim {PIN['version']}: {archive}")


def build_unix(source):
    env = os.environ.copy()
    env.setdefault("CFLAGS", "-O2")
    run([
        "./configure", "--with-features=huge", "--enable-channel", "--enable-terminal",
        "--enable-gui=no", "--without-x", "--disable-nls", "--disable-gpm",
        "--disable-sysmouse", "--disable-canberra", "--disable-libsodium", "--disable-netbeans",
        "--disable-pythoninterp", "--disable-python3interp", "--disable-luainterp",
        "--disable-perlinterp", "--disable-rubyinterp", "--disable-tclinterp",
        "--disable-mzschemeinterp",
    ], source, env)
    run(["make", f"-j{min(os.cpu_count() or 1, 4)}"], source, env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path, help="Shell-adjacent vim directory")
    parser.add_argument("--source-archive", type=Path, help="Use an existing Vim source archive")
    parser.add_argument("--iconv-archive", type=Path, help="Use an existing Windows libiconv binary archive")
    parser.add_argument("--iconv-source-archive", type=Path, help="Use an existing libiconv source archive")
    parser.add_argument("--verify-only", action="store_true", help="Check an already built bundle")
    parser.add_argument("--package", type=Path, help="Also write a prebuilt engine ZIP archive")
    args = parser.parse_args()
    if sys.version_info < (3, 12):
        parser.error("Building bundled Vim requires Python 3.12 or newer")
    output = args.output.resolve()
    if output == REPO or output == Path(output.anchor):
        parser.error("--output must be a dedicated Vim directory")
    if args.verify_only:
        verify_bundle(output)
        print(f"Verified Vim {PIN['version']}: {output}")
        if args.package:
            package_bundle(output, args.package)
        return
    cache = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")).resolve() / "vim-build"
    cache.mkdir(parents=True, exist_ok=True)
    archive = checked_archive(
        PIN["source_url"], PIN["source_sha256"],
        args.source_archive or cache / f"vim-{PIN['version']}.tar.gz", bool(args.source_archive))
    with tempfile.TemporaryDirectory(prefix="source-", dir=cache) as work:
        with tarfile.open(archive, "r:gz") as tar:
            tar.extractall(work, filter="data")
        source = Path(work) / f"vim-{PIN['commit']}"
        if os.name == "nt":
            build_windows(source)
        else:
            build_unix(source)
        staged = Path(work) / "bundle"
        staged.mkdir()
        executable = "vim.exe" if os.name == "nt" else "vim"
        shutil.copy2(source / "src" / executable, staged / executable)
        shutil.copytree(source / "runtime", staged / "runtime")
        shutil.copy2(source / "LICENSE", staged / "LICENSE")
        if os.name == "nt":
            stage_windows_iconv(staged, cache, args.iconv_archive, args.iconv_source_archive)
        (staged / "UPSTREAM.json").write_text(json.dumps(PIN, indent=2) + "\n", encoding="utf-8")
        verify_bundle(staged)
        output.parent.mkdir(parents=True, exist_ok=True)
        if output.exists():
            shutil.rmtree(output)
        shutil.copytree(staged, output)
    print(f"Built Vim {PIN['version']}: {output}")
    if args.package:
        package_bundle(output, args.package)


if __name__ == "__main__":
    main()
