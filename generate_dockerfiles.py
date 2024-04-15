import json
import subprocess
import os
import argparse
import time
import requests
import tomllib


parser = argparse.ArgumentParser()
parser.add_argument("--upload", action="store_true")
parser.add_argument("--skip_cache", action="store_true")
args = parser.parse_args()

# Array of Solana version mapped to rust version hashes
RUST_DOCKER_IMAGESHA_MAP = {
    "1.68.0": "79892de83d1af9109c47a4566a24a0b240348bb8c088f1bccc52645c4c70ec39",
    "1.69.0": "b7e0e2c6199fb5f309742c5eba637415c25ca2bed47fa5e80e274d4510ddfa3a",
    "1.72.1": "6562d50b62366d5b9db92b34c6684fab5bf3b9f627e59a863c9c0675760feed4",
    "1.73.0": "7ec316528af3582341280f667be6cfd93062a10d104f3b1ea72cd1150c46ef22",
    "1.75.0": "b7f381685785bb4192e53995d6ad1dec70954e682e18e06a4c8c02011ab2f32e",
}


RUST_VERSION_PLACEHOLDER = "$RUST_VERSION"
SOLANA_VERSION_PLACEHOLDER = "$SOLANA_VERSION"

base_dockerfile_text = f"""
FROM --platform=linux/amd64 rust@sha256:{RUST_VERSION_PLACEHOLDER}

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.solana.com/{SOLANA_VERSION_PLACEHOLDER}/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build

CMD /bin/bash
"""

output = subprocess.check_output(
    ["git", "ls-remote", "--tags", "https://github.com/solana-labs/solana"]
)

def check_version(version_str):
    try:
        # Ignore this one
        if version_str == "v1.14.0":
            return False
        [major, minor, _patch] = version_str.strip("v").split(".")
        # Ignore 1.15.x
        return int(major) >= 1 and int(minor) >= 14 and int(minor) != 15
    except Exception as e:
        return False

def get_toolchain(version_tag):
    if "v1.14" in version_tag:
        return "1.68.0"

    attempt = 0
    max_attempts = 5

    while attempt < max_attempts:
        url = "https://api.github.com/repos/solana-labs/solana/contents/rust-toolchain.toml?ref=tags/" + version_tag
        headers = {'Accept': 'application/vnd.github.v3.raw'}  # Fetch the raw file content

        response = requests.get(url, headers=headers)
        if response.status_code == 200:
            parsed_data = tomllib.loads(response.text)
            channel_version = parsed_data['toolchain']['channel']

            # Strict rate limit for unauthenticated requests
            time.sleep(2.5)

            return channel_version

        else:
            # Parse error message to json
            error = json.loads(response.text)

            print(response.text)

            if 'message' in error:
                if "rate limit exceeded" in error['message']:
                    wait = 5 + 2 ** attempt  # Exponential backoff factor
                    max_wait = 300  # Maximum waiting time in seconds
                    sleep_time = min(wait, max_wait)
                    print(f"Rate limit exceeded. Sleeping for {sleep_time} seconds.")
                    time.sleep(sleep_time)
                    attempt += 1
                elif error['message'] == "Not Found":
                    # If message is "Not Found" then default to 1.68.0
                    print("Using default rust version 1.68.0 for Solana version", version_tag)
                    return "1.68.0"
                else:
                    print("Failed to fetch the file")
                    print("Error message: " + error['message'])
                    return None

tags = list(
    filter(
        check_version,
        [
            elem.split("\t")[1].split("/")[-1]
            for elem in output.decode("utf-8").split("\n")
            if elem
        ],
    )
)

dockerfiles = {}

for release in tags:
    rust_version = get_toolchain(release)
    print(release + ", " + rust_version)

    if rust_version is None:
        print(f"Failed to fetch rust version for {release}")
        continue

    if rust_version not in RUST_DOCKER_IMAGESHA_MAP:
        print(f"Rust version {rust_version} not found in the map")
        continue

    dockerfile = base_dockerfile_text.replace(SOLANA_VERSION_PLACEHOLDER, release).lstrip("\n")
    dockerfile = dockerfile.replace(RUST_VERSION_PLACEHOLDER, RUST_DOCKER_IMAGESHA_MAP[rust_version])
    path = f"docker/{release}.Dockerfile"
    with open(path, "w") as f:
        f.write(dockerfile)
    dockerfiles[release] = path

if args.upload:
    digest_set = set()
    if not args.skip_cache:
        print("Fetching existing images")
        response = requests.get(
            "https://hub.docker.com/v2/namespaces/ellipsislabs/repositories/solana/tags?page_size=1000"
        )
        for result in response.json()["results"]:
            if result["name"] != "latest":
                try:
                    digest_set.add(result["name"])
                except Exception as e:
                    print(e)
                    continue

    print("Uploading all Dockerfiles")
    for tag, dockerfile in dockerfiles.items():
        # Strip the `v` from the tag to keep the versions consistent in Docker
        stripped_tag = tag.strip("v")
        if stripped_tag in digest_set:
            print(f"Already built image for {stripped_tag}, skipping")
            continue
        version_tag = f"solana:{stripped_tag}"
        print(version_tag)
        current_directory = os.getcwd()
        res = subprocess.call(
            f"docker build -t {version_tag} - < {current_directory}/{dockerfile}",
            shell=True,
        )
        if res == 0:
            subprocess.call(
                f"docker tag {version_tag} ellipsislabs/{version_tag}", shell=True
            )
            subprocess.call(f"docker push ellipsislabs/{version_tag}", shell=True)
        else:
            continue
