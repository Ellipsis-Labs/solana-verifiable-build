use anyhow::anyhow;
use cargo_lock::Lockfile;
use cargo_toml::Manifest;
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
pub mod api_client;
pub mod image_config;
pub mod api_models;
use image_config::IMAGE_MAP;

use crate::api_client::send_job_to_remote;

const MAINNET_GENESIS_HASH: &str = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";

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
        mount_directory: Option<String>,
        /// Path to mount to the workspace root
        #[clap(short, long="workspace-path")]
        workspace_directory: Option<String>,
        /// Which binary file to build (applies to repositories with multiple programs)
        #[clap(long)]
        library_name: Option<String>,
        /// Optionally specify a custom base docker image to use for building the program repository
        #[clap(short, long)]
        base_image: Option<String>,
        /// If the program requires cargo build-bpf (instead of cargo build-sbf), as for anchor program, set this flag
        #[clap(long, default_value = "false")]
        bpf: bool,
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
        /// Verify in current directory
        #[clap(long, default_value = "false")]
        current_dir: bool,
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
        /// Send the verify command to a remote machine
        #[clap(long, default_value = "false")]
        remote: bool,
        /// Relative path to the root directory or the source code repository from which to build the program
        /// This should be the directory that contains the workspace Cargo.toml and the Cargo.lock file
        #[clap(long, default_value = "")]
        mount_path: String,
        /// Relative path to the specific program workspace directory in the source code repository from which to build the program
        /// This should be the directory that contains the program's workspace Cargo.toml and the Cargo.lock file, if different from repo's workspace
        #[clap(long, default_value = "")]
        workspace_path: String,
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
        /// If the repo_url points to a repo that contains multiple programs, specify the name of the library name of the program to
        /// build and verify. You will also need to specify the library_name if the program is not part of the top-level Cargo.toml
        /// Otherwise it will be inferred from the Cargo.toml file
        #[clap(long)]
        library_name: Option<String>,
        /// If the program requires cargo build-bpf (instead of cargo build-sbf), as for an Anchor program, set this flag
        #[clap(long, default_value = "false")]
        bpf: bool,
        /// Verify in current directory
        #[clap(long, default_value = "false")]
        current_dir: bool,
        /// Arguments to pass to the underlying `cargo build-bpf` command
        #[clap(required = false, last = true)]
        cargo_args: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Handle SIGTERM and SIGINT gracefully by stopping the docker container
    let mut signals = Signals::new([SIGTERM, SIGINT])?;
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
            mount_directory,
            workspace_directory,
            library_name,
            base_image,
            bpf: bpf_flag,
            cargo_args,
        } => build(
            mount_directory,
            workspace_directory,
            library_name,
            base_image,
            bpf_flag,
            cargo_args,
            &mut container_id,
        ),
        SubCommand::VerifyFromImage {
            executable_path_in_image: executable_path,
            image,
            program_id,
            current_dir,
        } => verify_from_image(
            executable_path,
            image,
            args.url,
            program_id,
            current_dir,
            &mut temp_dir,
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
            remote,
            mount_path,
            workspace_path,
            repo_url,
            commit_hash,
            program_id,
            base_image,
            library_name,
            bpf: bpf_flag,
            cargo_args,
            current_dir,
        } => {
            verify_from_repo(
                remote,
                mount_path,
                workspace_path,
                args.url,
                repo_url,
                commit_hash,
                program_id,
                base_image,
                library_name,
                bpf_flag,
                cargo_args,
                current_dir,
                &mut container_id,
                &mut temp_dir,
            )
            .await
        }
    };

    if caught_signal.load(Ordering::Relaxed) || res.is_err() {
        if let Some(container_id) = container_id.clone().take() {
            println!("Stopping container {}", container_id);
            if std::process::Command::new("docker")
                .args(["kill", &container_id])
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
                .args(["-rf", &temp_dir])
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

pub fn get_binary_hash(program_data: Vec<u8>) -> String {
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

pub fn get_genesis_hash(url: Option<String>) -> anyhow::Result<String> {
    let client = get_client(url);
    let genesis_hash = client.get_genesis_hash()?;
    Ok(genesis_hash.to_string())
}


pub fn get_docker_resource_limits() -> Option<(String, String)> {
    let memory = std::env::var("SVB_DOCKER_MEMORY_LIMIT").ok();
    let cpus = std::env::var("SVB_DOCKER_CPU_LIMIT").ok();
    if memory.is_some() || cpus.is_some() {
        println!("Using docker resource limits: memory: {:?}, cpus: {:?}", memory, cpus);
    } else {
        // Print message to user that they can set these environment variables to limit docker resources
        println!("No Docker resource limits are set.");
        println!("You can set the SVB_DOCKER_MEMORY_LIMIT and SVB_DOCKER_CPU_LIMIT environment variables to limit Docker resources.");
        println!("For example: SVB_DOCKER_MEMORY_LIMIT=2g SVB_DOCKER_CPU_LIMIT=2.");
    }
    memory.zip(cpus)
}

pub fn build(
    mount_directory: Option<String>,
    workspace_root: Option<String>,
    library_name: Option<String>,
    base_image: Option<String>,
    bpf_flag: bool,
    cargo_args: Vec<String>,
    container_id_opt: &mut Option<String>,
) -> anyhow::Result<()> {
    let mount_path = mount_directory.unwrap_or(
        std::env::current_dir()?
            .as_os_str()
            .to_str()
            .ok_or_else(|| anyhow::Error::msg("Invalid path string"))?
            .to_string(),
    );
    println!("Mounting path: {}", mount_path);

    let workspace_path = workspace_root.unwrap_or(mount_path.clone());
    println!("Workspace path: {}", workspace_path);

    let lockfile = format!("{}/Cargo.lock", workspace_path);
    if !std::path::Path::new(&lockfile).exists() {
        println!("Mount directory must contain a Cargo.lock file");
        return Err(anyhow!(format!("No lockfile found at {}", lockfile)));
    }

    let build_command = if bpf_flag { "build-bpf" } else { "build-sbf" };

    let (major, minor, patch) = get_pkg_version_from_cargo_lock("solana-program", &lockfile)?;

    let mut solana_version: Option<String> = None;
    let  image: String = base_image.unwrap_or_else(|| {
        if bpf_flag {
            // Use this for backwards compatibility with anchor verified builds
            solana_version = Some("v1.13.5".to_string());
            "projectserum/build@sha256:75b75eab447ebcca1f471c98583d9b5d82c4be122c470852a022afcf9c98bead".to_string()
        } else if let Some(digest) = IMAGE_MAP.get(&(major, minor, patch)) {
                println!("Found docker image for Solana version {}.{}.{}", major, minor, patch);
                solana_version = Some(format!("v{}.{}.{}", major, minor, patch));
                format!("ellipsislabs/solana@{}", digest)
            } else {
                println!("Unable to find docker image for Solana version {}.{}.{}", major, minor, patch);
                let prev = IMAGE_MAP.range(..(major, minor, patch)).next_back();
                let next = IMAGE_MAP.range((major, minor, patch)..).next();
                let (version, digest) = if let Some((version, digest)) = prev {
                    (version, digest)
                } else if let Some((version, digest)) = next {
                    (version, digest)
                } else {
                    println!("Unable to find backup docker image for Solana version {}.{}.{}", major, minor, patch);
                    std::process::exit(1);
                };
                println!("Using backup docker image for Solana version {}.{}.{}", version.0, version.1, version.2);
                solana_version = Some(format!("v{}.{}.{}", version.0, version.1, version.2));
                format!("ellipsislabs/solana@{}", digest)
            }
    });

    let mut manifest_path = None;

    let relative_build_path = std::process::Command::new("find")
        .args([&mount_path, "-name", "Cargo.toml"])
        .output()
        .map_err(|e| {
            anyhow::format_err!(
                "Failed to find Cargo.toml files in root directory: {}",
                e.to_string()
            )
        })
        .and_then(|output| {
            for p in String::from_utf8(output.stdout)?.split("\n") {
                match get_lib_name_from_cargo_toml(p) {
                    Ok(name) => {
                        if name == library_name.clone().unwrap_or_default() {
                            manifest_path = Some(p.to_string().replace(&mount_path, ""));
                            return Ok(p
                                .to_string()
                                .replace("Cargo.toml", "")
                                .replace(&mount_path, ""));
                        }
                    }
                    Err(_) => {
                        continue;
                    }
                }
            }
            Err(anyhow!("No Cargo.toml files found"))
        })
        .unwrap_or_else(|_| "".to_string());

    let workdir = std::process::Command::new("docker")
        .args(["run", "--rm", &image, "pwd"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| anyhow::format_err!("Failed to get workdir: {}", e.to_string()))
        .and_then(|output| parse_output(output.stdout))?;

    println!("Workdir: {}", workdir);

    let build_path = format!("{}/{}", workdir, relative_build_path);
    println!("Building program at {}", build_path);

    let manifest_path_filter = manifest_path
        .clone()
        .map(|m| vec!["--manifest-path".to_string(), format!("{}/{}", workdir, m)])
        .unwrap_or_else(Vec::new);

    if manifest_path.is_some() {
        println!(
            "Building manifest path: {}/{}",
            workdir,
            manifest_path.unwrap()
        );
    }

    // change directory to program/build dir
    let mount_params = format!("{}:{}", mount_path, workdir);
    let container_id = {
        let mut cmd = std::process::Command::new("docker");
            cmd.args(["run", "--rm", "-v", &mount_params, "-dit"]);
            cmd.stderr(Stdio::inherit());

        if let Some((memory_limit, cpu_limit)) = get_docker_resource_limits() {
            cmd.arg("--memory").arg(memory_limit).arg("--cpus").arg(cpu_limit);
        }

        let output = cmd
            .args([&image, "bash"])
            .output()
            .map_err(|e| anyhow!("Docker build failed: {}", e.to_string()))?;

        parse_output(output.stdout)?
    };

    // Set the container id so we can kill it later if the process is interrupted
    container_id_opt.replace(container_id.clone());

    // Solana v1.17 uses Rust 1.73, which defaults to the sparse registry, making
    // this fetch unnecessary, but requires us to omit the "frozen" argument
    let locked_args = if major == 1 && minor < 17 {
        // First, we resolve the dependencies and cache them in the Docker container
        // ARM processors running Linux have a bug where the build fails if the dependencies are not preloaded.
        // Running the build without the pre-fetch will cause the container to run out of memory.
        // This is a workaround for that issue.
        std::process::Command::new("docker")
            .args(["exec", &container_id])
            .args([
                "cargo",
                "--config",
                "net.git-fetch-with-cli=true",
                "fetch",
                "--locked",
            ])
            .stderr(Stdio::inherit())
            .stdout(Stdio::inherit())
            .output()?;
        println!("Finished fetching build dependencies");

        ["--frozen", "--locked"].as_slice()
    } else {
        // To be totally safe, force the build to use the sparse registry
        [
            "--config",
            "registries.crates-io.protocol=\"sparse\"",
            "--locked",
        ]
        .as_slice()
    };

    std::process::Command::new("docker")
        .args(["exec", "-w", &build_path, &container_id])
        .args(["cargo", build_command])
        .args(["--"])
        .args(locked_args)
        .args(manifest_path_filter)
        .args(cargo_args)
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .output()?;

    println!("Finished building program");
    println!("Program Solana version: v{}.{}.{}", major, minor, patch);

    if let Some(solana_version) = solana_version {
        println!("Docker image Solana version: {}", solana_version);
    }

    if let Some(program_name) = library_name {
        let executable_path = std::process::Command::new("find")
            .args([
                &format!("{}/target/deploy", workspace_path),
                "-name",
                &format!("{}.so", program_name),
            ])
            .output()
            .map_err(|e| anyhow!("Failed to find program: {}", e.to_string()))
            .and_then(|output| parse_output(output.stdout))?;
        let executable_hash = get_file_hash(&executable_path)?;
        println!("{}", executable_hash);
    }
    std::process::Command::new("docker")
        .args(["kill", &container_id])
        .output()?;
    Ok(())
}

pub fn verify_from_image(
    executable_path: String,
    image: String,
    network: Option<String>,
    program_id: Pubkey,
    current_dir: bool,
    temp_dir: &mut Option<String>,
    container_id_opt: &mut Option<String>,
) -> anyhow::Result<()> {
    println!(
        "Verifying image: {:?}, on network {:?} against program ID {}",
        image, network, program_id
    );
    println!("Executable path in container: {:?}", executable_path);
    println!(" ");

    let workdir = std::process::Command::new("docker")
        .args(["run", "--rm", &image, "pwd"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| anyhow::format_err!("Failed to get workdir: {}", e.to_string()))
        .and_then(|output| parse_output(output.stdout))?;

    println!("Workdir: {}", workdir);


    let container_id = {
        let mut cmd = std::process::Command::new("docker");
            cmd.args(["run", "--rm", "-dit"]);
            cmd.stderr(Stdio::inherit());

        if let Some((memory_limit, cpu_limit)) = get_docker_resource_limits() {
            cmd.arg("--memory").arg(memory_limit).arg("--cpus").arg(cpu_limit);
        }

        let output = cmd
            .args([&image])
            .output()
            .map_err(|e| anyhow!("Docker build failed: {}", e.to_string()))?;
        parse_output(output.stdout)?
    };

    container_id_opt.replace(container_id.clone());

    let uuid = Uuid::new_v4().to_string();

    // Create a temporary directory to clone the repo into
    let verify_dir = if current_dir {
        format!(
            "{}/.{}",
            std::env::current_dir()?
                .as_os_str()
                .to_str()
                .ok_or_else(|| anyhow::Error::msg("Invalid path string"))?,
            uuid.clone()
        )
    } else {
        "/tmp".to_string()
    };

    temp_dir.replace(verify_dir.clone());

    let program_filepath = format!("{}/program.so", verify_dir);
    std::process::Command::new("docker")
        .args([
            "cp",
            format!("{}:{}/{}", container_id, workdir, executable_path).as_str(),
            program_filepath.as_str(),
        ])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| anyhow::format_err!("Failed to copy executable file {}", e.to_string()))?;

    let executable_hash: String = get_file_hash(program_filepath.as_str())?;
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
        .map_err(|e| anyhow::format_err!("Docker kill failed: {}", e.to_string()))?;

    std::process::Command::new("rm")
        .args([program_filepath])
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

#[allow(clippy::too_many_arguments)]
pub async fn verify_from_repo(
    remote: bool,
    relative_mount_path: String,
    relative_workspace_path: String,
    connection_url: Option<String>,
    repo_url: String,
    commit_hash: Option<String>,
    program_id: Pubkey,
    base_image: Option<String>,
    library_name_opt: Option<String>,
    bpf_flag: bool,
    cargo_args: Vec<String>,
    current_dir: bool,
    container_id_opt: &mut Option<String>,
    temp_dir_opt: &mut Option<String>,
) -> anyhow::Result<()> {
    if remote {
        let genesis_hash = get_genesis_hash(connection_url)?;
        if genesis_hash != MAINNET_GENESIS_HASH {
            return Err(anyhow!("Remote verification only works with mainnet. Please omit the --remote flag to verify locally."));
        }

        println!("Sending verify command to remote machine...");
        send_job_to_remote(
            &repo_url,
            &commit_hash,
            &program_id,
            &library_name_opt,
            bpf_flag,
            relative_mount_path,
            base_image,
            cargo_args,
        )
        .await?;
        return Ok(());
    }

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
            "{}/.{}",
            std::env::current_dir()?
                .as_os_str()
                .to_str()
                .ok_or_else(|| anyhow::Error::msg("Invalid path string"))?,
            uuid.clone()
        )
    } else {
        format!("/tmp/solana-verify/{}", uuid)
    };

    temp_dir_opt.replace(verify_dir.clone());

    let verify_tmp_root_path = format!("{}/{}", verify_dir, base_name);
    println!("Cloning repo into: {}", verify_tmp_root_path);

    std::process::Command::new("git")
        .args(["clone", &repo_url, &verify_tmp_root_path])
        .stdout(Stdio::inherit())
        .output()?;

    // Checkout a specific commit hash, if provided
    if let Some(commit_hash) = commit_hash {
        let result = std::process::Command::new("git")
            .args(["-C", &verify_tmp_root_path])
            .args(["checkout", &commit_hash])
            .output()
            .map_err(|e| anyhow!("Failed to checkout commit hash: {:?}", e));
        if result.is_ok() {
            println!("Checked out commit hash: {}", commit_hash);
        } else {
            std::process::Command::new("rm")
                .args(["-rf", verify_dir.as_str()])
                .output()?;
            Err(anyhow!("Encountered error in git setup: {:?}", result))?;
        }
    }

    // Get the absolute build path to the solana program directory to build inside docker
    let mount_path = PathBuf::from(verify_tmp_root_path.clone()).join(relative_mount_path);
    println!("Build path: {:?}", mount_path);

    let library_name = match library_name_opt {
        Some(p) => p,
        None => { 
                std::process::Command::new("find")
                    .args([mount_path.to_str().unwrap(), "-name", "Cargo.toml"])
                    .output()
                    .map_err(|e| {
                        anyhow::format_err!(
                            "Failed to find Cargo.toml files in root directory: {}",
                            e.to_string()
                        )
                    })
                    .and_then(|output| {
                        let mut options = vec![];
                        for path in String::from_utf8(output.stdout)?.split("\n") {
                            match get_lib_name_from_cargo_toml(path) {
                                Ok(name) => {
                                    options.push(name);
                                }
                                Err(_) => {
                                    continue;
                                }
                            }
                        }
                        if options.len() != 1 {
                            println!(
                                "Found multiple possible targets in root directory: {:?}",
                                options
                            );
                            println!(
                                "Please explicitly specify the target with the --package-name <name> option",
                            );
                            Err(anyhow::format_err!(
                                "Failed to find unique Cargo.toml file in root directory"
                            ))
                        } else {
                            Ok(options[0].clone())
                        }
                    })?
        }
    };
    println!("Verifying program: {}", library_name);

    // Get the absolute build path to the solana program directory to build inside docker
    let workspace_path = PathBuf::from(verify_tmp_root_path.clone()).join(relative_workspace_path);
    println!("Workspace path: {:?}", workspace_path);

    let result = build_and_verify_repo(
        mount_path.to_str().unwrap().to_string(),
        workspace_path.to_str().unwrap().to_string(),
        base_image,
        bpf_flag,
        library_name,
        connection_url,
        program_id,
        cargo_args,
        container_id_opt,
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
            println!("Program hash matches ✅");
        } else {
            println!("Program hashes do not match ❌");
        }

        Ok(())
    } else {
        Err(anyhow!("Error verifying program. {:?}", result))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_and_verify_repo(
    mount_path: String,
    workspace_path: String,
    base_image: Option<String>,
    bpf_flag: bool,
    library_name: String,
    connection_url: Option<String>,
    program_id: Pubkey,
    cargo_args: Vec<String>,
    container_id_opt: &mut Option<String>,
) -> anyhow::Result<(String, String)> {
    // Build the code using the docker container
    let executable_filename = format!("{}.so", &library_name);
    build(
        Some(mount_path.clone()),
        Some(workspace_path.clone()),
        Some(library_name),
        base_image,
        bpf_flag,
        cargo_args,
        container_id_opt,
    )?;

    // Get the hash of the build
    let executable_path = std::process::Command::new("find")
        .args([
            &format!("{}/target/deploy", workspace_path),
            "-name",
            executable_filename.as_str(),
        ])
        .output()
        .map_err(|e| anyhow::format_err!("Failed to find executable file {}", e.to_string()))
        .and_then(|output| parse_output(output.stdout))?;
    println!("Executable file found at path: {:?}", executable_path);
    let build_hash = get_file_hash(&executable_path)?;

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

pub fn get_pkg_version_from_cargo_lock(
    package_name: &str,
    cargo_lock_file: &str,
) -> anyhow::Result<(u32, u32, u32)> {
    let lockfile = Lockfile::load(cargo_lock_file)?;
    let res = lockfile
        .packages
        .iter()
        .filter(|pkg| pkg.name.to_string() == *package_name)
        .filter_map(|pkg| {
            let version = pkg.version.clone().to_string();
            let version_parts: Vec<&str> = version.split(".").collect();
            if version_parts.len() == 3 {
                let major = version_parts[0].parse::<u32>().unwrap_or(0);
                let minor = version_parts[1].parse::<u32>().unwrap_or(0);
                let patch = version_parts[2].parse::<u32>().unwrap_or(0);
                return Some((major, minor, patch));
            }
            None
        })
        .next()
        .ok_or_else(|| anyhow!("Failed to parse solana-program version from Cargo.lock"))?;
    Ok(res)
}

pub fn get_lib_name_from_cargo_toml(cargo_toml_file: &str) -> anyhow::Result<String> {
    let manifest = Manifest::from_path(cargo_toml_file)?;
    let lib = manifest
        .lib
        .ok_or_else(|| anyhow!("Failed to parse lib from Cargo.toml"))?;
    lib.name
        .ok_or_else(|| anyhow!("Failed to parse lib name from Cargo.toml"))
}

pub fn get_pkg_name_from_cargo_toml(cargo_toml_file: &str) -> Option<String> {
    let manifest = Manifest::from_path(cargo_toml_file).ok()?;
    let pkg = manifest.package?;
    Some(pkg.name)
}
