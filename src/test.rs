mod tests {
    use anyhow::Context;
    use regex::Regex;
    use std::io::Write;
    use std::process::Stdio;

    fn test_verify_program_hash_helper(expected_hash: &str, args: &[&str]) -> anyhow::Result<()> {
        let mut child = std::process::Command::new("./target/debug/solana-verify")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to execute solana-verify command")?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(b"n")?;
        }

        let output = child
            .wait_with_output()
            .context("Failed to wait for solana-verify command")?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Command failed: {}", error);
        }

        // Print the last 10 lines of the output
        let output_str = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = output_str.split('\n').collect();
        let last_10_lines: Vec<String> =
            lines.iter().rev().take(10).map(|s| s.to_string()).collect();
        println!("Last 10 lines of output:\n{}", last_10_lines.join("\n"));

        let re = Regex::new(r"Executable Program Hash from repo: ([a-f0-9]{64})")
            .context("Failed to compile regex")?;

        let program_hash = re
            .captures(&output_str)
            .context("Could not find program hash in output")?
            .get(1)
            .context("Invalid capture group")?
            .as_str();

        assert_eq!(
            program_hash, expected_hash,
            "Program hash {} does not match expected value {}",
            program_hash, expected_hash
        );

        Ok(())
    }

    #[test]
    fn test_phoenix_v1() -> anyhow::Result<()> {
        const EXPECTED_HASH: &str =
            "6877a5b732b3494b828a324ec846d526d962223959534dbaf4209e0da3b2d6a9";
        let args: Vec<&str> =  "verify-from-repo -um --program-id PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY https://github.com/Ellipsis-Labs/phoenix-v1".split(" ").collect();
        test_verify_program_hash_helper(EXPECTED_HASH, &args)?;
        Ok(())
    }

    #[test]
    fn test_squads_v3() -> anyhow::Result<()> {
        const EXPECTED_HASH: &str =
            "72da599d9ee14b2a03a23ccfa6f06d53eea4a00825ad2191929cbd78fb69205c";
        let args: Vec<&str> = "verify-from-repo https://github.com/Squads-Protocol/squads-mpl --commit-hash c95b7673d616c377a349ca424261872dfcf8b19d --program-id SMPLecH534NA9acpos4G6x7uf3LWbCAwZQE9e8ZekMu -um --library-name squads_mpl --bpf".split(" ").collect();
        test_verify_program_hash_helper(EXPECTED_HASH, &args)?;
        Ok(())
    }

    #[test]
    fn test_drift_v2() -> anyhow::Result<()> {
        const EXPECTED_HASH: &str =
            "e31d58edeabc3c30bf6f2aa60bfaa5e492b41ec203e9006404b463e5adee5828";
        let args: Vec<&str> = "verify-from-repo -um --program-id dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH https://github.com/drift-labs/protocol-v2 --commit-hash 110d3ff4f8ba07c178d69f9bfc7b30194fac56d6 --library-name drift".split(" ").collect();
        test_verify_program_hash_helper(EXPECTED_HASH, &args)?;
        Ok(())
    }

    #[test]
    fn test_marginfi_v2() -> anyhow::Result<()> {
        const EXPECTED_HASH: &str =
            "890d68f48f96991016222b1fcbc2cc81b8ef2dcbf280c44fe378c523c108fad5";
        let args: Vec<&str> = "verify-from-repo -um --program-id MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA https://github.com/mrgnlabs/marginfi-v2 --commit-hash d33e649e415c354cc2a1e3c49131725552d69ba0 --library-name marginfi".split(" ").collect();
        test_verify_program_hash_helper(EXPECTED_HASH, &args)?;
        Ok(())
    }

    #[test]
    fn test_games_preset() -> anyhow::Result<()> {
        const EXPECTED_HASH: &str =
            "668ff275819d9276362c6a2636d2a392afe224296e815481b94474785f490025";
        let args: Vec<&str> = "verify-from-repo -um --program-id MkabCfyUD6rBTaYHpgKBBpBo5qzWA2pK2hrGGKMurJt https://github.com/solana-developers/solana-game-preset --commit-hash eaf772fd1f21fe03a9974587f5680635e970be38 --mount-path program".split(" ").collect();
        test_verify_program_hash_helper(EXPECTED_HASH, &args)?;
        Ok(())
    }

    #[test]
    fn test_agave_2_1() -> anyhow::Result<()> {
        const EXPECTED_HASH: &str =
            "29e7713aa3c48e242e2847bc031fe2a03eb61aae5ecaec8728131e16934de465";
        let args: Vec<&str> = "verify-from-repo https://github.com/Woody4618/verify-2-1 --program-id kGYz2q2WUYCXhKpgUF4AMR3seDA9eg8sbirP5dhbyhy --commit-hash e0f138fb58b669791c823f44f878cb3547a92a26".split(" ").collect();
        test_verify_program_hash_helper(EXPECTED_HASH, &args)?;
        Ok(())
    }

    #[test]
    fn test_local_example() -> anyhow::Result<()> {
        const EXPECTED_HASH: &str =
            "08d91368d349c2b56c712422f6d274a1e8f1946ff2ecd1dc3efc3ebace52a760";

        let args: Vec<&str> = "build ./examples/hello_world".split(" ").collect();
        let child = std::process::Command::new("./target/debug/solana-verify")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to execute solana-verify command")?;

        let output = child
            .wait_with_output()
            .context("Failed to wait for solana-verify command")?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Command failed: {}", error);
        }

        let args: Vec<&str> =
            "get-executable-hash ./examples/hello_world/target/deploy/hello_world.so"
                .split(" ")
                .collect();
        let child = std::process::Command::new("./target/debug/solana-verify")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to execute solana-verify command")?;

        let output = child
            .wait_with_output()
            .context("Failed to wait for solana-verify command")?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("Command failed: {}", error);
        }

        let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            hash, EXPECTED_HASH,
            "Program hash {} does not match expected value {}",
            hash, EXPECTED_HASH
        );
        Ok(())
    }

    #[test]
    fn test_verify_from_image() -> anyhow::Result<()> {
        let args: Vec<&str> = "verify-from-image -e examples/hello_world/target/deploy/hello_world.so -i ellipsislabs/hello_world_verifiable_build:latest -p 2ZrriTQSVekoj414Ynysd48jyn4AX6ZF4TTJRqHfbJfn".split(" ").collect();
        let child = std::process::Command::new("./target/debug/solana-verify")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to execute solana-verify command")?;

        let output = child
            .wait_with_output()
            .context("Failed to wait for solana-verify command")?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Command failed: {}", error);
        }
        Ok(())
    }
}
