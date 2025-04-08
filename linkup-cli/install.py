#!/usr/bin/env python3

import sys

python_version = sys.version_info
if python_version.major < 3 or python_version.minor < 2:
    print(f"Minimum required Python version is 3.2. Current one: {sys.version.split(' ')[0]}")
    exit(1)

import argparse
import json
import os
import re
import shutil
import tarfile
import urllib.request
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any, Optional, Tuple, List

LINKUP_BIN_PATH = Path.home() / ".linkup" / "bin"

class Shell(Enum):
    bash = "bash"
    zsh = "zsh"
    fish = "fish"


    @staticmethod
    def from_str(value: str) -> Optional["Shell"]:
        value_lower = value.lower()

        if value_lower == "bash":
            return Shell.bash
        elif value_lower == "zsh":
            return Shell.zsh
        elif value_lower == "fish":
            return Shell.fish
        else:
            return None


    def add_to_profile_command(self) -> Optional[str]:
        if self == Shell.bash:
            return f"echo 'export PATH=$PATH:{LINKUP_BIN_PATH}' >> {Path.home()}/.bashrc"
        elif self == Shell.zsh:
            return f"echo 'export PATH=$PATH:{LINKUP_BIN_PATH}' >> {Path.home()}/.zshrc"
        elif self == Shell.fish:
            return f"echo 'set -gx PATH $PATH {LINKUP_BIN_PATH}' >> {Path.home()}/.config/fish/config.fish"
        else:
            return None


class OS(Enum):
    MacOS = "apple-darwin"
    Linux = "unknown-linux-gnu"


class Arch(Enum):
    x86_64 = "x86_64"
    arm64 = "aarch64"


class Channel(Enum):
    stable = "stable"
    beta = "beta"


@dataclass
class GithubReleaseAsset:
    name: str
    browser_download_url: str


@dataclass
class GithubRelease:
    tag_name: str
    prerelease: bool
    assets: List[GithubReleaseAsset]

    @staticmethod
    def from_json(obj: Any) -> "GithubRelease":
        assets = [
            GithubReleaseAsset(
                name=asset["name"],
                browser_download_url=asset["browser_download_url"],
            )
            for asset in obj["assets"]
        ]

        return GithubRelease(tag_name=obj["tag_name"], prerelease=obj["prerelease"], assets=assets)


@dataclass
class Context:
    channel: Channel
    fetch_os: Optional[OS] = None
    fetch_arch: Optional[Arch] = None
    release_data: Optional[GithubRelease] = None


def command_exists(cmd: str) -> bool:
    return shutil.which(cmd) is not None


def check_dependencies() -> None:
    if not command_exists("cloudflared"):
        print("WARN: 'cloudflared' is not installed. Please install it before installing Linkup.")
        print("More info: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/")
        sys.exit(1)


def detect_platform() -> Tuple[OS, Arch]:
    os_name = os.uname().sysname
    arch = os.uname().machine

    if os_name.startswith("Darwin"):
        fetch_os = OS.MacOS
    elif os_name.startswith("Linux"):
        fetch_os = OS.Linux
    else:
        print(f"Unsupported OS: {os_name}")
        sys.exit(1)

    if arch in ("arm64", "aarch64"):
        fetch_arch = Arch.arm64
    elif arch == "x86_64":
        fetch_arch = Arch.x86_64
    else:
        print(f"Unsupported Arch: {arch}")
        sys.exit(1)

    return fetch_os, fetch_arch


def get_release_data(channel: Channel) -> GithubRelease:
    if channel == Channel.beta:
        print("Looking for the latest beta version...")
        releases = list_releases()

        pre_releases = [r for r in releases if r.prerelease]
        if not pre_releases:
            print("No pre-releases found. Falling back to latest stable release.")

            return get_latest_stable_release()
        else:
            print(f"Found pre-release version: {pre_releases[0].tag_name}")

            return pre_releases[0]
    else:
        return get_latest_stable_release()


def list_releases() -> List[GithubRelease]:
    req = urllib.request.Request(
        "https://api.github.com/repos/mentimeter/linkup/releases",
        headers={
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28"
        }
    )

    with urllib.request.urlopen(req) as response:
        return [GithubRelease.from_json(release) for release in json.load(response)]


def get_latest_stable_release() -> GithubRelease:
    req = urllib.request.Request(
        "https://api.github.com/repos/mentimeter/linkup/releases/latest",
        headers={
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28"
        }
    )

    with urllib.request.urlopen(req) as response:
        return GithubRelease.from_json(json.load(response))


def download_and_extract(user_os: OS, user_arch: Arch, channel: Channel, release: GithubRelease) -> None:
    print(f"Latest release on {channel.name} channel: {release.tag_name}.")
    print(f"Looking for asset for {user_os.value}/{user_arch.value}...")
    asset_pattern = re.compile(rf"linkup-.+-{user_arch.value}-{user_os.value}\.tar\.gz$")

    download_url = next(
        (
            asset.browser_download_url
            for asset in release.assets if asset_pattern.match(asset.name)
        ),
        None,
    )

    if not download_url:
        print("Could not find matching tarball in the release assets.")
        sys.exit(1)

    print(f"Downloading: {download_url}")
    local_tar_path = Path("/tmp") / Path(download_url).name

    with urllib.request.urlopen(download_url) as response, open(local_tar_path, "wb") as out_file:
        shutil.copyfileobj(response, out_file)

    print(f"Decompressing {local_tar_path}")
    with tarfile.open(local_tar_path, "r:gz") as tar:
        tar.extractall(path="/tmp")

    LINKUP_BIN_PATH.mkdir(parents=True, exist_ok=True)
    shutil.move("/tmp/linkup", LINKUP_BIN_PATH / "linkup")
    os.chmod(LINKUP_BIN_PATH / "linkup", 0o755)

    print(f"Linkup installed at {LINKUP_BIN_PATH / 'linkup'}")
    local_tar_path.unlink()


def setup_path() -> None:
    if str(LINKUP_BIN_PATH) in os.environ.get("PATH", "").split(":"):
        return

    print(f"\nTo start using Linkup, add '{LINKUP_BIN_PATH}' to your PATH.")

    shell = Shell.from_str(os.path.basename(os.environ.get("SHELL", "")))
    if shell is None:
        return

    print(f"Since you are using {shell.name}, you can run the following to add to your profile:")
    print(f"\n  {shell.add_to_profile_command()}")
    print("\nThen restart your shell.");


def parse_arguments(args: List[str]) -> Context:
    parser = argparse.ArgumentParser(description="Install Linkup CLI")

    parser.add_argument(
        "--channel",
        choices=["stable", "beta"],
        default="stable",
        help="Release channel to use (default: stable)"
    )

    parsed = parser.parse_args(args)
    channel = Channel[parsed.channel]

    return Context(channel=channel)


def main() -> None:
    if command_exists("linkup"):
        print("Linkup is already installed. To update it, run 'linkup update'.")
        sys.exit(0)

    context = parse_arguments(sys.argv[1:])

    check_dependencies()

    user_os, user_arch = detect_platform()
    release = get_release_data(context.channel)
    download_and_extract(user_os, user_arch, context.channel, release)

    setup_path()

    print("Linkup installation complete! 🎉")


if __name__ == "__main__":
    main()
