import subprocess
import os
import argparse
import requests
import tomllib
import re

parser = argparse.ArgumentParser()
parser.add_argument("--upload", action="store_true")
parser.add_argument("--skip_cache", action="store_true")
parser.add_argument("--version")
args = parser.parse_args()

# Array of Solana version mapped to rust version hashes
RUST_DOCKER_IMAGESHA_MAP = {
    "1.68.0": "sha256:79892de83d1af9109c47a4566a24a0b240348bb8c088f1bccc52645c4c70ec39"
}

RUST_VERSION_PLACEHOLDER = "$RUST_VERSION"
SOLANA_VERSION_PLACEHOLDER = "$SOLANA_VERSION"
AGAVE_VERSION_PLACEHOLDER = "$AGAVE_VERSION"

# Dockerfile template for Solana versions >= 1.15
base_dockerfile_sol = f"""
FROM --platform=linux/amd64 rust@{RUST_VERSION_PLACEHOLDER}

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.solana.com/{SOLANA_VERSION_PLACEHOLDER}/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build

CMD /bin/bash
"""

# Dockerfile template for Solana versions < 1.15
base_dockerfile_sol_pre15 = f"""
FROM --platform=linux/amd64 rust@{RUST_VERSION_PLACEHOLDER}

RUN apt-get update && apt-get install -qy git gnutls-bin curl

# Download and modify the Solana install script to install the specified version
RUN curl -sSfL "https://release.solana.com/v1.18.20/install" -o solana_install.sh && \\
    chmod +x solana_install.sh && \\
    sed -i "s/^SOLANA_INSTALL_INIT_ARGS=.*/SOLANA_INSTALL_INIT_ARGS={SOLANA_VERSION_PLACEHOLDER}/" solana_install.sh && \\
    ./solana_install.sh && \\
    rm solana_install.sh

ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build
CMD /bin/bash
"""

# Dockerfile template for Agave
base_dockerfile_agave = f"""
FROM --platform=linux/amd64 rust@{RUST_VERSION_PLACEHOLDER}

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.anza.xyz/{AGAVE_VERSION_PLACEHOLDER}/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build

CMD /bin/bash
"""

# Skipping these because the are not valid releases or where yanked
SKIPPED_VERSIONS = {
    "v1.15", "v1.14.0", "v1.10.0", "v1.10.16", "v1.10.18",
    "v1.10.27", "v1.10.36", "v1.10.37", "v1.11.7", "v1.11.8", "v1.11.9",
    "v1.13.0",
}

# Determine release information for Solana or Agave
def get_release_info(version_tag):
    """
    Determines if a version is a Solana or Agave release and provides relevant info.
    Returns a dictionary with base_dockerfile_text, version_placeholder, and the URL for the toolchain.
    """
    version_parts = version_tag.strip("v").split(".")
    if not all(part.isdigit() for part in version_parts):
        print(f"Skipping non-numeric tag: {version_tag}")
        return None

    major, minor, patch = map(int, version_parts)

    if major == 1 and minor == 15:
        print(f"Skipping yanked v1.15.x release: {version_tag}")
        return None

    if version_tag in SKIPPED_VERSIONS:
        print(f"Skipping yanked release: {version_tag}")
        return None

    # It would also be possible to generate more images for older versions but is it necessary?
    if major == 1 and minor < 10:
        print(f"Skipping all releases before 10 release: {version_tag}")
        return None
    
    # All releases 14 and below have a broken installer, so we need to use a custom script with a newer installer
    if (major == 1 and minor < 15):
        release_info = {
            "base_dockerfile_text": base_dockerfile_sol_pre15,
            "version_placeholder": SOLANA_VERSION_PLACEHOLDER,
            "url": f"https://raw.githubusercontent.com/solana-labs/solana/{version_tag}/rust-toolchain.toml"
        }
    # Everything from 1.15.0 to 1.18.24 we want to get from solana labs 
    elif (major == 1 and minor >= 15) and not (minor == 18 and patch >= 24):
        release_info = {
            "base_dockerfile_text": base_dockerfile_sol,
            "version_placeholder": SOLANA_VERSION_PLACEHOLDER,
            "url": f"https://raw.githubusercontent.com/solana-labs/solana/{version_tag}/rust-toolchain.toml"
        }
    #Everything after we need to get from anza 
    elif (major == 1 and minor == 18 and patch >= 24) or major >= 2:
        release_info = {
            "base_dockerfile_text": base_dockerfile_agave,
            "version_placeholder": AGAVE_VERSION_PLACEHOLDER,
            "url": f"https://raw.githubusercontent.com/anza-xyz/agave/{version_tag}/rust-toolchain.toml"
        }
    else:
        print(f"Skipping {version_tag} as it does not meet Solana or Agave criteria.")
        return None
    return release_info

# Function to get Solana releases
def get_solana_releases():
    output = subprocess.check_output(
        ["git", "ls-remote", "--tags", "https://github.com/solana-labs/solana"]
    )
    tags = [
        elem.split("\t")[1].split("/")[-1]
        for elem in output.decode("utf-8").split("\n")
        if elem
    ]
    return tags

# Function to get Agave releases
def get_agave_releases():
    output = subprocess.check_output(
        ["git", "ls-remote", "--tags", "https://github.com/anza-xyz/agave"]
    )
    tags = [
        elem.split("\t")[1].split("/")[-1]
        for elem in output.decode("utf-8").split("\n")
        if elem
    ]
    return tags

# Function to get Rust toolchain for each release
def get_toolchain(version_tag):
    if "v1.14" in version_tag:
        return "1.68.0"

    release_info = get_release_info(version_tag)
    if release_info is None:
        return None

    url = release_info["url"]
    response = requests.get(url, headers={"Accept": "application/vnd.github.v3.raw"})
    if response.status_code == 200:
        parsed_data = tomllib.loads(response.text)
        return parsed_data["toolchain"]["channel"]
    print(f"Failed to fetch rust-toolchain.toml for {version_tag}")
    # Fallback to rust-version.ci for older versions
    print(f"Attempting to retrieve Rust version from rust-version.ci for {version_tag}")
    return get_rust_version_from_ci(version_tag)

def get_rust_version_from_ci(version_tag):
    url = f"https://raw.githubusercontent.com/solana-labs/solana/{version_tag}/ci/rust-version.sh"
    response = requests.get(url)
    if response.status_code == 200:
        match = re.search(r'stable_version=(\d+\.\d+\.\d+)', response.text)
        if match:
            return match.group(1)
        else:
            print(f"No stable Rust version found in {version_tag}")
    else:
        print(f"Failed to fetch rust-version.sh for {version_tag}")
    return None

# Process releases and generate Dockerfiles
def process_releases(releases):
    for release in releases:
        release_info = get_release_info(release)
        if release_info is None:
            continue

        base_dockerfile_text = release_info["base_dockerfile_text"]
        version_placeholder = release_info["version_placeholder"]

        rust_version = get_toolchain(release)
        print(f"Generating Dockerfile for {release} with Rust version {rust_version}")

        if rust_version is None:
            print(f"Skipping {release} due to missing Rust version.")
            continue

        if rust_version not in RUST_DOCKER_IMAGESHA_MAP and rust_version != "1.68.0":
            response = requests.get(
                f"https://hub.docker.com/v2/namespaces/library/repositories/rust/tags/{rust_version}"
            )
            if response.status_code == 200:
                for image in response.json()["images"]:
                    if image["architecture"] == "amd64":
                        RUST_DOCKER_IMAGESHA_MAP[rust_version] = image["digest"]
                        break
                if rust_version not in RUST_DOCKER_IMAGESHA_MAP:
                    print(f"Failed to fetch Rust image for {rust_version}")
                    continue

        dockerfile = base_dockerfile_text.replace(
            version_placeholder, release
        ).replace(
            RUST_VERSION_PLACEHOLDER, RUST_DOCKER_IMAGESHA_MAP[rust_version]
        ).lstrip("\n")

        path = f"docker/{release}.Dockerfile"
        if os.path.exists(path):
            with open(path, "r") as f:
                if f.read() != dockerfile:
                    dirty_set.add(release.strip("v"))
        else:
            dirty_set.add(release.strip("v"))
        with open(path, "w") as f:
            f.write(dockerfile)
        dockerfiles[release] = path

# Main execution
solana_releases = get_solana_releases()
agave_releases = get_agave_releases()

dockerfiles = {}
dirty_set = set()

process_releases(solana_releases)
process_releases(agave_releases)

print(RUST_DOCKER_IMAGESHA_MAP)


digest_set = set()
if not args.skip_cache:
    print("Fetching existing images")
    response = requests.get(
        "https://hub.docker.com/v2/namespaces/solanafoundation/repositories/solana-verifiable-build/tags?page_size=1000"
    )
    for result in response.json()["results"]:
        print(result)
        if result["name"] != "latest":
            try:
                digest_set.add(result["name"])
            except Exception as e:
                print(e)
                continue

if args.upload:
    print("Uploading all Dockerfiles")
    for tag, dockerfile in dockerfiles.items():
        # Strip the `v` from the tag to keep the versions consistent in Docker
        stripped_tag = tag.strip("v")

        (major, minor, patch) = stripped_tag.split(".")

        print(stripped_tag, args.version)

        force_build = False
        if args.version is not None:
            ver = args.version.split(".")
            if len(ver) == 2:
                a_major, a_minor = ver
                a_patch = patch
            if len(ver) == 3:
                a_major, a_minor, a_patch = ver
            if major != a_major or minor != a_minor or a_patch != patch:
                print(f"Skipping {stripped_tag}")
                continue
            force_build = True

        if (
            stripped_tag in digest_set
            and stripped_tag not in dirty_set
            and not force_build
        ):
            print(f"Already built image for {stripped_tag}, skipping")
            continue
        if stripped_tag in dirty_set:
            print(f"Dockerfile for {stripped_tag} needs to be modified")
        version_tag = f"solana:{stripped_tag}"
        print(version_tag)
        current_directory = os.getcwd()
        res = subprocess.call(
            f"docker build -t {version_tag} - < {current_directory}/{dockerfile}",
            shell=True,
        )
        if res == 0:
            subprocess.call(
                f"docker tag {version_tag} solanafoundation/solana-verifiable-build/{version_tag}", shell=True
            )
            subprocess.call(f"docker push solanafoundation/solana-verifiable-build/{version_tag}", shell=True)
        else:
            continue