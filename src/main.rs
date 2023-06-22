use anyhow::anyhow;
use clap::{Parser, Subcommand};
use signal_hook::{
    consts::{SIGINT, SIGTERM},
    iterator::Signals,
};
use solana_cli_config::{Config, CONFIG_FILE};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    pubkey::Pubkey,
};
use std::{
    io::Read,
    path::PathBuf,
    process::Stdio,
    sync::atomic::AtomicBool,
    sync::{atomic::Ordering, Arc},
};
use uuid::Uuid;

pub fn get_network(network_str: &str) -> &str {
    match network_str {
        "devnet" | "dev" | "d" => "https://api.devnet.solana.com",
        "mainnet" | "main" | "m" | "mainnet-beta" => "https://api.mainnet-beta.solana.com",
        "localnet" | "localhost" | "l" | "local" => "http://localhost:8899",
        _ => network_str,
    }
}

#[derive(Parser, Debug)]
#[clap(author = "Ellipsis", version, about)]
struct Arguments {
    #[clap(subcommand)]
    subcommand: SubCommand,
    /// Optionally include your RPC endpoint. Use "local", "dev", "main" for default endpoints. Defaults to your Solana CLI config file.
    #[clap(global = true, short, long)]
    url: Option<String>,
}

#[derive(Subcommand, Debug)]
enum SubCommand {
    /// Deterministically build the program in an Docker container
    Build {
        /// Path to mount to the docker image
        mount_dir: Option<String>,
        /// Which binary file to build (applies to repositories with multiple programs)
        #[clap(long)]
        package_name: Option<String>,
        /// Optionally specify a custom base docker image to use for building the program repository
        #[clap(short, long)]
        base_image: Option<String>,
        /// If the program requires cargo build-bpf (instead of cargo build-sbf), as for anchor program, set this flag
        #[clap(long, default_value = "false")]
        bpf_flag: bool,
        /// Docker workdir
        #[clap(long, default_value = "build")]
        workdir: String,
        /// Arguments to pass to the underlying `cargo build-bpf` command
        #[clap(required = false, last = true)]
        cargo_args: Vec<String>,
    },
    /// Verifies a cached build from a docker image
    VerifyFromImage {
        /// Path to the executable solana program within the source code repository in the docker image
        #[clap(short, long)]
        executable_path_in_image: String,
        /// Image that contains the source code to be verified
        #[clap(short, long)]
        image: String,
        /// The Program ID of the program to verify
        #[clap(short, long)]
        program_id: Pubkey,
    },
    /// Get the hash of a program binary from an executable file
    GetExecutableHash {
        /// Path to the executable solana program
        filepath: String,
    },
    /// Get the hash of a program binary from the deployed on-chain program
    GetProgramHash {
        /// The Program ID of the program to verify
        program_id: Pubkey,
    },
    /// Get the hash of a program binary from the deployed buffer address
    GetBufferHash {
        /// Address of the buffer account containing the deployed program data
        buffer_address: Pubkey,
    },
    /// Builds and verifies a program from a given repository URL and a program ID
    VerifyFromRepo {
        /// Path to the executable solana program within the source code repository if the program is not part of the top-level Cargo.toml
        #[clap(short, long, default_value = "")]
        solana_program_path: String,
        /// The HTTPS URL of the repo to clone
        repo_url: String,
        /// Optional commit hash to checkout
        #[clap(long)]
        commit_hash: Option<String>,
        /// The Program ID of the program to verify
        #[clap(long)]
        program_id: Pubkey,
        /// Optionally specify a custom base docker image to use for building the program repository
        #[clap(short, long)]
        base_image: Option<String>,
        /// If the repo_url points to a repo that contains multiple programs, specify the name of the program to build and verify
        #[clap(long, default_value = "*")]
        package_name: String,
        /// If the program requires cargo build-bpf (instead of cargo build-sbf), as for anchor program, set this flag
        #[clap(long, default_value = "false")]
        bpf_flag: bool,
        /// Docker workdir
        #[clap(long, default_value = "build")]
        workdir: String,
        /// Verify in current directory
        #[clap(long, default_value = "false")]
        current_dir: bool,
        /// Arguments to pass to the underlying `cargo build-bpf` command
        #[clap(required = false, last = true)]
        cargo_args: Vec<String>,
    },
}

fn main() -> anyhow::Result<()> {
    // Handle SIGTERM and SIGINT gracefully by stopping the docker container
    let mut signals = Signals::new(&[SIGTERM, SIGINT])?;
    let mut container_id: Option<String> = None;
    let mut temp_dir: Option<String> = None;
    let caught_signal = Arc::new(AtomicBool::new(false));

    let caught_signal_clone = caught_signal.clone();
    let handle = signals.handle();
    std::thread::spawn(move || {
        for _ in signals.forever() {
            caught_signal_clone.store(true, Ordering::Relaxed);
            break;
        }
    });

    let args = Arguments::parse();
    let res = match args.subcommand {
        SubCommand::Build {
            // mount directory
            mount_dir,
            // program build directory
            package_name,
            base_image,
            bpf_flag,
            workdir,
            cargo_args,
        } => {
            build(
                mount_dir,
                package_name,
                base_image,
                bpf_flag,
                workdir,
                cargo_args,
                &mut container_id,
            )?;
            Ok(())
        }
        SubCommand::VerifyFromImage {
            executable_path_in_image: executable_path,
            image,
            program_id,
        } => verify_from_image(
            executable_path,
            image,
            args.url,
            program_id,
            &mut container_id,
        ),
        SubCommand::GetExecutableHash { filepath } => {
            let program_hash = get_file_hash(&filepath)?;
            println!("{}", program_hash);
            Ok(())
        }
        SubCommand::GetBufferHash { buffer_address } => {
            let buffer_hash = get_buffer_hash(args.url, buffer_address)?;
            println!("{}", buffer_hash);
            Ok(())
        }
        SubCommand::GetProgramHash { program_id } => {
            let program_hash = get_program_hash(args.url, program_id)?;
            println!("{}", program_hash);
            Ok(())
        }
        SubCommand::VerifyFromRepo {
            solana_program_path,
            repo_url,
            commit_hash,
            program_id,
            base_image,
            package_name,
            bpf_flag,
            workdir,
            cargo_args,
            current_dir,
        } => {
            // Get source code from repo_url
            let base_name = std::process::Command::new("basename")
                .arg(&repo_url)
                .output()
                .map_err(|e| anyhow!("Failed to get basename of repo_url: {:?}", e))
                .and_then(|output| parse_output(output.stdout))?;

            let uuid = Uuid::new_v4().to_string();

            // Create a temporary directory to clone the repo into
            let verify_dir = if current_dir {
                format!(
                    "{}/{}",
                    std::env::current_dir()?
                        .as_os_str()
                        .to_str()
                        .ok_or_else(|| anyhow::Error::msg("Invalid path string"))?
                        .to_string(),
                    uuid.clone()
                )
            } else {
                format!("/tmp/solana-verify/{}", uuid)
            };

            temp_dir.replace(verify_dir.clone());

            let verify_tmp_file_path = format!("{}/{}", verify_dir, base_name);

            std::process::Command::new("git")
                .args(["clone", &repo_url, &verify_tmp_file_path])
                .output()?;

            // Checkout a specific commit hash, if provided
            if let Some(commit_hash) = commit_hash {
                let result = std::process::Command::new("cd")
                    .arg(&verify_tmp_file_path)
                    .output()
                    .and_then(|_| {
                        std::process::Command::new("git")
                            .args(["checkout", &commit_hash])
                            .output()
                    });
                if result.is_ok() {
                    println!("Checked out commit hash: {}", commit_hash);
                } else {
                    std::process::Command::new("rm")
                        .args(["-rf", verify_dir.as_str()])
                        .output()?;
                    Err(anyhow!("Failed to checkout commit hash: {:?}", result))?;
                }
            }

            // Get the absolute build path to the solana program directory to build inside docker
            let build_path = PathBuf::from(verify_tmp_file_path.clone()).join(solana_program_path);
            println!("Build path: {:?}", build_path);

            let result = verify_from_repo(
                build_path.to_str().unwrap().to_string(),
                base_image,
                bpf_flag,
                package_name,
                args.url,
                program_id,
                workdir,
                cargo_args,
                &mut container_id,
            );

            // Cleanup no matter the result
            std::process::Command::new("rm")
                .args(["-rf", &verify_dir])
                .output()?;

            // Compare hashes or return error
            if let Ok((build_hash, program_hash)) = result {
                println!("Executable Program Hash from repo: {}", build_hash);
                println!("On-chain Program Hash: {}", program_hash);

                if build_hash == program_hash {
                    println!("Program hash matches");
                } else {
                    println!("Program hash does not match");
                }

                Ok(())
            } else {
                Err(anyhow!("Error verifying program. {:?}", result))
            }
        }
    };

    if caught_signal.load(Ordering::Relaxed) {
        if let Some(container_id) = container_id.clone().take() {
            println!("Stopping container {}", container_id);
            if std::process::Command::new("docker")
                .args(&["kill", &container_id])
                .output()
                .is_err()
            {
                println!("Failed to close docker container");
            } else {
                println!("Stopped container {}", container_id)
            }
        }
        if let Some(temp_dir) = temp_dir.clone().take() {
            println!("Removing temp dir {}", temp_dir);
            if std::process::Command::new("rm")
                .args(&["-rf", &temp_dir])
                .output()
                .is_err()
            {
                println!("Failed to remove temp dir");
            } else {
                println!("Removed temp dir {}", temp_dir);
            }
        }
    }
    handle.close();
    res
}

pub fn get_client(url: Option<String>) -> RpcClient {
    let config = match CONFIG_FILE.as_ref() {
        Some(config_file) => Config::load(config_file).unwrap_or_else(|_| {
            println!("Failed to load config file: {}", config_file);
            Config::default()
        }),
        None => Config::default(),
    };
    let url = &get_network(&url.unwrap_or(config.json_rpc_url)).to_string();
    RpcClient::new(url)
}

fn get_binary_hash(program_data: Vec<u8>) -> String {
    let buffer = program_data
        .into_iter()
        .rev()
        .skip_while(|&x| x == 0)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    sha256::digest(&buffer[..])
}

pub fn get_file_hash(filepath: &str) -> Result<String, std::io::Error> {
    let mut f = std::fs::File::open(filepath)?;
    let metadata = std::fs::metadata(filepath)?;
    let mut buffer = vec![0; metadata.len() as usize];
    f.read_exact(&mut buffer)?;
    Ok(get_binary_hash(buffer))
}

pub fn get_buffer_hash(url: Option<String>, buffer_address: Pubkey) -> anyhow::Result<String> {
    let client = get_client(url);
    let offset = UpgradeableLoaderState::size_of_buffer_metadata();
    let account_data = client.get_account_data(&buffer_address)?[offset..].to_vec();
    let program_hash = get_binary_hash(account_data);
    Ok(program_hash)
}

pub fn get_program_hash(url: Option<String>, program_id: Pubkey) -> anyhow::Result<String> {
    let client = get_client(url);
    let program_buffer =
        Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id()).0;
    let offset = UpgradeableLoaderState::size_of_programdata_metadata();
    let account_data = client.get_account_data(&program_buffer)?[offset..].to_vec();
    let program_hash = get_binary_hash(account_data);
    Ok(program_hash)
}

pub fn build(
    mount_path: Option<String>,
    package_name: Option<String>,
    base_image: Option<String>,
    bpf_flag: bool,
    workdir: String,
    cargo_args: Vec<String>,
    container_id_opt: &mut Option<String>,
) -> anyhow::Result<()> {
    let path = mount_path.unwrap_or(
        std::env::current_dir()?
            .as_os_str()
            .to_str()
            .ok_or_else(|| anyhow::Error::msg("Invalid path string"))?
            .to_string(),
    );
    println!("Mounting path: {}", path);
    let image = base_image.unwrap_or_else(|| "ellipsislabs/solana:latest".to_string());

    let build_command = if bpf_flag { "build-bpf" } else { "build-sbf" };

    let package_filter = package_name
        .clone()
        .map(|pkg| vec!["-p".to_string(), pkg])
        .unwrap_or_else(|| vec![]);

    // change directory to program/build dir
    let mount_params = format!("{}:/{}", path, workdir);
    let container_id = std::process::Command::new("docker")
        .args(["run", "--rm", "-v", &mount_params, "-dit", &image])
        .args(["cargo", build_command, "--", "--locked", "--frozen"])
        .args(package_filter)
        .args(cargo_args)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| anyhow::format_err!("Docker build failed: {}", e.to_string()))
        .and_then(|output| parse_output(output.stdout))?;

    // Set the container id so we can kill it later if the process is interrupted
    container_id_opt.replace(container_id.clone());

    std::process::Command::new("docker")
        .args(["logs", "--follow", &container_id])
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .output()?;

    if let Some(program_name) = package_name {
        let executable_path = std::process::Command::new("find")
            .args([
                &format!("{}/target/deploy", path),
                "-name",
                &format!("{}.so", program_name),
            ])
            .output()
            .map_err(|e| anyhow!("Failed to find program: {}", e.to_string()))
            .and_then(|output| parse_output(output.stdout))?;
        let executable_hash = get_file_hash(&executable_path)?;
        println!("Executable hash: {}", executable_hash);
    }
    Ok(())
}

pub fn verify_from_image(
    executable_path: String,
    image: String,
    network: Option<String>,
    program_id: Pubkey,
    container_id_opt: &mut Option<String>,
) -> anyhow::Result<()> {
    println!(
        "Verifying image: {:?}, on network {:?} against program ID {}",
        image, network, program_id
    );
    println!("Executable path in container: {:?}", executable_path);
    println!(" ");

    let container_id = std::process::Command::new("docker")
        .args(["run", "--rm", "-dit", image.as_str()])
        .output()
        .map_err(|e| anyhow::format_err!("Failed to run image {}", e.to_string()))
        .and_then(|output| parse_output(output.stdout))?;

    container_id_opt.replace(container_id.clone());

    std::process::Command::new("docker")
        .args([
            "cp",
            format!("{}:/build/{}", container_id, executable_path).as_str(),
            "/tmp/program.so",
        ])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| anyhow::format_err!("Failed to copy executable file {}", e.to_string()))?;

    let executable_hash: String = get_file_hash("/tmp/program.so")?;
    let client = get_client(network);
    let program_buffer =
        Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id()).0;
    let offset = UpgradeableLoaderState::size_of_programdata_metadata();
    let account_data = &client.get_account_data(&program_buffer)?[offset..];
    let program_hash = get_binary_hash(account_data.to_vec());
    println!("Executable hash: {}", executable_hash);
    println!("Program hash: {}", program_hash);

    // Cleanup docker and rm file
    std::process::Command::new("docker")
        .args(["kill", container_id.as_str()])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| anyhow::format_err!("Docker build failed: {}", e.to_string()))?;

    std::process::Command::new("rm")
        .args(["/tmp/program.so"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| {
            anyhow::format_err!("Failed to remove temp program file: {}", e.to_string())
        })?;

    if program_hash != executable_hash {
        println!("Executable hash mismatch");
        return Err(anyhow::Error::msg("Executable hash mismatch"));
    } else {
        println!("Executable matches on-chain program data ✅");
    }
    Ok(())
}

pub fn verify_from_repo(
    base_repo_path: String,
    base_image: Option<String>,
    bpf_flag: bool,
    package_name: String,
    connection_url: Option<String>,
    program_id: Pubkey,
    workdir: String,
    cargo_args: Vec<String>,
    container_id_opt: &mut Option<String>,
) -> anyhow::Result<(String, String)> {
    // Build the code using the docker container
    build(
        Some(base_repo_path.clone()),
        Some(package_name.clone()),
        base_image,
        bpf_flag,
        workdir,
        cargo_args,
        container_id_opt,
    )?;

    let executable_filename = format!("{}.so", package_name);

    // Get the hash of the build
    println!(
        "Looking for executable name {} at path: {}/target/deploy",
        executable_filename, base_repo_path
    );
    let executable_path = std::process::Command::new("find")
        .args([
            &format!("{}/target/deploy", base_repo_path),
            "-name",
            executable_filename.as_str(),
        ])
        .output()
        .map_err(|e| anyhow::format_err!("Failed to find executable file {}", e.to_string()))
        .and_then(|output| parse_output(output.stdout))?;
    println!("Executable file found at path: {:?}", executable_path);
    let build_hash = get_file_hash(&executable_path)?;
    println!("Build hash: {}", build_hash);

    // Get the hash of the deployed program
    println!(
        "Fetching on-chain program data for program ID: {}",
        program_id,
    );
    let program_hash = get_program_hash(connection_url, program_id)?;

    Ok((build_hash, program_hash))
}

pub fn parse_output(output: Vec<u8>) -> anyhow::Result<String> {
    let parsed_output = String::from_utf8(output)?
        .strip_suffix("\n")
        .ok_or_else(|| anyhow!("Failed to parse output"))?
        .to_string();
    Ok(parsed_output)
}
