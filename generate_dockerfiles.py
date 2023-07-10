import subprocess
import os
import argparse
import requests

parser = argparse.ArgumentParser()
parser.add_argument("--upload", action="store_true")
parser.add_argument("--skip_cache", action="store_true")
args = parser.parse_args()

VERSION_PLACEHOLDER = "$VERSION"

base_dockerfile_text = f"""
FROM --platform=linux/amd64 rust@sha256:79892de83d1af9109c47a4566a24a0b240348bb8c088f1bccc52645c4c70ec39

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.solana.com/{VERSION_PLACEHOLDER}/install)"
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
        return int(major) >= 1 and int(minor) >= 14
    except Exception as e:
        return False


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
    dockerfile = base_dockerfile_text.replace(VERSION_PLACEHOLDER, release).lstrip("\n")
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
