# Solana Verified Builds

This repository demonstrates how to implement verified builds for Solana programs. Verified builds ensure that your deployed program matches exactly with your public source code, promoting transparency and security in the Solana ecosystem.

## What are Verified Builds?

Verified builds allow developers and users to verify that a deployed Solana program matches its source code. This verification:

- Ensures program authenticity
- Promotes transparency
- Builds user trust
- Makes source code discoverable

## Quick Start

1. Install prerequisites:

   - Docker
   - Cargo
   - Solana Verify CLI (`cargo install solana-verify`)

2. Build your program:

```bash
solana-verify build
```

3. Deploy and verify:

```bash
# Deploy
solana program deploy -u $NETWORK_URL target/deploy/$PROGRAM_LIB_NAME.so --program-id $PROGRAM_ID

# Verify against repository -> upload your build data on chain
solana-verify verify-from-repo -u $NETWORK_URL --program-id $PROGRAM_ID https://github.com/$REPO_PATH

# Trigger a remote job
solana-verify remote submit-job --program-id $PROGRAM_ID --uploader $THE_PUBKEY_THAT_UPLOADED_YOUR_BUILD_DATA
```

## Documentation

For detailed instructions and best practices, please refer to the [official Solana documentation on verified builds](https://solana.com/developers/guides/advanced/verified-builds).

## Security Considerations

While verified builds enhance transparency, they should not be considered a complete security solution. Always:

- Review the source code
- Use trusted build environments
- Consider using governance solutions for program upgrades

For responsible disclosure of bugs related to verified builds CLI, please email maintainers@ellipsislabs.xyz with a detailed description of the attack vector.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
