import subprocess
import os
import argparse

parser = argparse.ArgumentParser()
parser.add_argument("--upload", action="store_true")
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
    dockerfile = base_dockerfile_text.replace(VERSION_PLACEHOLDER, release).strip("\n")
    path = f"docker/{release}.Dockerfile"
    with open(path, "w") as f:
        f.write(dockerfile)
    dockerfiles[release] = path

if args.upload:
    print("Uploading all Dockerfiles")
    for tag, dockerfile in dockerfiles.items():
        # Strip the `v` from the tag to keep the versions consistent in Docker
        version_tag = f"solana:{tag.strip('v')}"
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
