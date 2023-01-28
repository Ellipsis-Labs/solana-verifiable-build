# Solana Verifiable Build CLI

## Installation

In order for this CLI to work properly, you must have `docker` installed on your computer. Follow the steps here: https://docs.docker.com/engine/install/ to install Docker (based on your platform)

Once the installation is complete, make sure that the server has been started: (https://docs.docker.com/config/daemon/start/)

To install the verifier cli, run the following in your shell:

```

```

## Example Walkthrough

After installing the CLI, we can test the program verification against the following immutable mainnet program: `2ZrriTQSVekoj414Ynysd48jyn4AX6ZF4TTJRqHfbJfn`

Check it out here: https://solana.fm/address/2ZrriTQSVekoj414Ynysd48jyn4AX6ZF4TTJRqHfbJfn?cluster=mainnet-qn1

To get the hash of the of this program, we can run the following:

```
verifier-cli get-program-hash -p 2ZrriTQSVekoj414Ynysd48jyn4AX6ZF4TTJRqHfbJfn
```

Which will return the following hash:

```
627a5b29a06179d08ac5eab2c085703e59decbe6
```

By default, this command will strip any trailing zeros away from the program executable file and run the sha1 algorithm against it to compute the hash. If we knew the exact length of this executable, we could run:

```
verifier-cli get-program-hash -p 2ZrriTQSVekoj414Ynysd48jyn4AX6ZF4TTJRqHfbJfn -l 20608
```

And this would be the output:

```
79061f569f4b23728b3412153dedf5c5d4109257
```

To manually verify this build, one could run the following from the root of this repository:

```
cd examples/hello_world
# This command takes a long time because it is building the binary in a Docker container
verifier-cli build

```

```
verifier-cli get-executable-hash -f target/deploy/hello_world.so

```

And you will see that this returns the same value as the `get-program-hash` command with the custom length

```

79061f569f4b23728b3412153dedf5c5d4109257

```

To get the stripped version, run:

```

verifier-cli get-executable-hash -f target/deploy/hello_world.so --strip

```
