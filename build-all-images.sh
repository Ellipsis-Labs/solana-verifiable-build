#!/bin/bash

# Change directory to where the Dockerfiles are located
cd ./docker

# Iterate over each Dockerfile in the directory
for dockerfile in *; do
    # Check if the file is actually a Dockerfile
    if [ -f "$dockerfile" ] && [ "${dockerfile##*.}" == "Dockerfile" ]; then
        # Extract image name from Dockerfile name
        image_name="${dockerfile%Dockerfile}"
        # Remove the last character from the image name
        image_name="${image_name%?}"
        # Interpolate image_name with "solana."
        image_name="solana.$image_name"
        echo "Building image: $image_name"
        # Build the Docker image
        docker build -t "$image_name" -f "$dockerfile" .
    fi
done
