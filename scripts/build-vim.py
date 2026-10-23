#!/usr/bin/env python3
"""Build the pinned, unmodified Vim executable and runtime beside Tundra."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request


REPO = Path(__file__).resolve().parent.parent
PIN = json.loads((REPO / "packaging/vim.json").read_text(encoding="utf-8"))


def run(arguments, cwd, env=None):
    subprocess.run(arguments, cwd=cwd, env=env, check=True)


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
    for relative in ("runtime/defaults.vim", "runtime/doc/help.txt", "LICENSE", "UPSTREAM.json"):
        path = directory / relative
        if not path.is_file() or path.stat().st_size == 0:
            raise RuntimeError(f"Incomplete Vim bundle: missing {relative}")
    if json.loads((directory / "UPSTREAM.json").read_text(encoding="utf-8")) != PIN:
        raise RuntimeError("Bundled Vim does not match packaging/vim.json")


def build_windows(source):
    arguments = [
        "nmake", "/nologo", "-f", "Make_mvc.mak", "FEATURES=HUGE", "GUI=no",
        "VIMDLL=no", "CHANNEL=yes", "TERMINAL=yes", "ICONV=no", "GETTEXT=no", "SOUND=no",
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


def build_unix(source, universal):
    env = os.environ.copy()
    env.setdefault("CFLAGS", "-O2")
    if universal:
        if platform.system() != "Darwin":
            raise RuntimeError("--macos-universal requires a macOS build host")
        for name in ("CFLAGS", "LDFLAGS"):
            env[name] = env.get(name, "") + " -arch arm64 -arch x86_64"
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
    parser.add_argument("--source-archive", type=Path, help="Use a downloaded archive without network access")
    parser.add_argument("--macos-universal", action="store_true")
    parser.add_argument("--verify-only", action="store_true", help="Check an already built bundle")
    args = parser.parse_args()
    if sys.version_info < (3, 12):
        parser.error("Building bundled Vim requires Python 3.12 or newer")
    output = args.output.resolve()
    if output == REPO or output == Path(output.anchor):
        parser.error("--output must be a dedicated Vim directory")
    if args.verify_only:
        verify_bundle(output)
        print(f"Verified Vim {PIN['version']}: {output}")
        return
    cache = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")).resolve() / "vim-build"
    cache.mkdir(parents=True, exist_ok=True)
    archive = args.source_archive or cache / f"vim-{PIN['version']}.tar.gz"
    if not archive.exists():
        if args.source_archive:
            parser.error(f"Source archive does not exist: {archive}")
        partial = archive.with_suffix(".download")
        with urllib.request.urlopen(PIN["source_url"], timeout=60) as response, partial.open("wb") as file:
            shutil.copyfileobj(response, file)
        partial.replace(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != PIN["source_sha256"]:
        raise RuntimeError(f"Vim source SHA-256 mismatch: {archive}")
    with tempfile.TemporaryDirectory(prefix="source-", dir=cache) as work:
        with tarfile.open(archive, "r:gz") as tar:
            tar.extractall(work, filter="data")
        source = Path(work) / f"vim-{PIN['commit']}"
        if os.name == "nt":
            if args.macos_universal:
                parser.error("--macos-universal requires a macOS build host")
            build_windows(source)
        else:
            build_unix(source, args.macos_universal)
        staged = Path(work) / "bundle"
        staged.mkdir()
        executable = "vim.exe" if os.name == "nt" else "vim"
        shutil.copy2(source / "src" / executable, staged / executable)
        shutil.copytree(source / "runtime", staged / "runtime")
        shutil.copy2(source / "LICENSE", staged / "LICENSE")
        (staged / "UPSTREAM.json").write_text(json.dumps(PIN, indent=2) + "\n", encoding="utf-8")
        verify_bundle(staged)
        if args.macos_universal:
            run(["lipo", "-verify_arch", "arm64", "x86_64", str(staged / executable)], source)
        output.parent.mkdir(parents=True, exist_ok=True)
        if output.exists():
            shutil.rmtree(output)
        shutil.copytree(staged, output)
    print(f"Built Vim {PIN['version']}: {output}")


if __name__ == "__main__":
    main()
