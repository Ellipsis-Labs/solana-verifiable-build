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
        /// Which binary file to build (applies to repositories with multiple programs)
        #[clap(long)]
        library_name: Option<String>,
        /// Optionally specify a custom base docker image to use for building the program repository
        #[clap(short, long)]
        base_image: Option<String>,
        /// If the program requires cargo build-bpf (instead of cargo build-sbf), as for anchor program, set this flag
        #[clap(long, default_value = "false")]
        bpf: bool,
        /// Docker workdir
        #[clap(long)]
        workdir: Option<String>,
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
        /// Docker workdir
        #[clap(long, default_value = "build")]
        workdir: String,
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
        /// Relative path to the root directory or the source code repository from which to build the program
        /// This should be the directory that contains the workspace Cargo.toml and the Cargo.lock file
        #[clap(long, default_value = "")]
        mount_path: String,
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
        /// Docker workdir
        #[clap(long)]
        workdir: Option<String>,
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
            mount_directory,
            library_name,
            base_image,
            bpf: bpf_flag,
            workdir,
            cargo_args,
        } => build(
            mount_directory,
            library_name,
            base_image,
            bpf_flag,
            workdir,
            cargo_args,
            &mut container_id,
        ),
        SubCommand::VerifyFromImage {
            executable_path_in_image: executable_path,
            image,
            program_id,
            workdir,
            current_dir,
        } => verify_from_image(
            executable_path,
            image,
            args.url,
            program_id,
            workdir,
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
            mount_path,
            repo_url,
            commit_hash,
            program_id,
            base_image,
            library_name,
            bpf: bpf_flag,
            workdir,
            cargo_args,
            current_dir,
        } => verify_from_repo(
            mount_path,
            args.url,
            repo_url,
            commit_hash,
            program_id,
            base_image,
            library_name,
            bpf_flag,
            workdir,
            cargo_args,
            current_dir,
            &mut container_id,
            &mut temp_dir,
        ),
    };

    if caught_signal.load(Ordering::Relaxed) || res.is_err() {
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

pub fn build(
    mount_directory: Option<String>,
    library_name: Option<String>,
    base_image: Option<String>,
    bpf_flag: bool,
    workdir_opt: Option<String>,
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

    let lockfile = format!("{}/Cargo.lock", mount_path);
    if !std::path::Path::new(&lockfile).exists() {
        println!("Mount directory must contain a Cargo.lock file");
        return Err(anyhow!(format!("No lockfile found at {}", lockfile)));
    }

    let is_anchor = std::path::Path::new(&format!("{}/Anchor.toml", mount_path)).exists();
    let build_command = if bpf_flag || is_anchor {
        "build-bpf"
    } else {
        "build-sbf"
    };

    let image = base_image.unwrap_or_else(|| {
        if bpf_flag || is_anchor {
            "projectserum/build:v0.26.0"
        } else {
            "ellipsislabs/solana:latest"
        }
        .to_string()
    });

    let mut package_name = None;

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
                            package_name = get_pkg_name_from_cargo_toml(p);
                            println!("Package name: {:?}", package_name);
                            println!("Cargo path: {}", p.replace(&mount_path, ""));
                            return Ok(p
                                .to_string()
                                .replace("/Cargo.toml", "")
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

    let workdir = workdir_opt.unwrap_or_else(|| {
        if bpf_flag || is_anchor {
            "workdir"
        } else {
            "build"
        }
        .to_string()
    });

    let build_path = format!("/{}/{}", workdir, relative_build_path);
    println!("Building program at {}", build_path);

    let package_filter = package_name
        .clone()
        .map(|pkg| vec!["-p".to_string(), pkg])
        .unwrap_or_else(|| vec![]);

    if package_name.is_some() {
        println!("Building package: {}", package_name.unwrap());
    }

    // change directory to program/build dir
    let mount_params = format!("{}:/{}", mount_path, workdir);
    let container_id = std::process::Command::new("docker")
        .args(["run", "--rm", "-v", &mount_params, "-dit", &image, "bash"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| anyhow::format_err!("Docker build failed: {}", e.to_string()))
        .and_then(|output| parse_output(output.stdout))?;

    // Set the container id so we can kill it later if the process is interrupted
    container_id_opt.replace(container_id.clone());

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
    std::process::Command::new("docker")
        .args(["exec", "-w", &build_path, &container_id])
        .args(["cargo", build_command, "--", "--locked", "--frozen"])
        .args(package_filter)
        .args(cargo_args)
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .output()?;

    if let Some(program_name) = library_name {
        let executable_path = std::process::Command::new("find")
            .args([
                &format!("{}/target/deploy", mount_path),
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
        .args(&["kill", &container_id])
        .output()?;
    Ok(())
}

pub fn verify_from_image(
    executable_path: String,
    image: String,
    network: Option<String>,
    program_id: Pubkey,
    workdir: String,
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

    let container_id = std::process::Command::new("docker")
        .args(["run", "--rm", "-dit", image.as_str()])
        .output()
        .map_err(|e| anyhow::format_err!("Failed to run image {}", e.to_string()))
        .and_then(|output| parse_output(output.stdout))?;

    container_id_opt.replace(container_id.clone());

    let uuid = Uuid::new_v4().to_string();

    // Create a temporary directory to clone the repo into
    let verify_dir = if current_dir {
        format!(
            "{}/.{}",
            std::env::current_dir()?
                .as_os_str()
                .to_str()
                .ok_or_else(|| anyhow::Error::msg("Invalid path string"))?
                .to_string(),
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
            format!("{}:/{}/{}", container_id, workdir, executable_path).as_str(),
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

pub fn verify_from_repo(
    relative_mount_path: String,
    connection_url: Option<String>,
    repo_url: String,
    commit_hash: Option<String>,
    program_id: Pubkey,
    base_image: Option<String>,
    library_name_opt: Option<String>,
    bpf_flag: bool,
    workdir: Option<String>,
    cargo_args: Vec<String>,
    current_dir: bool,
    container_id_opt: &mut Option<String>,
    temp_dir_opt: &mut Option<String>,
) -> anyhow::Result<()> {
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
                .ok_or_else(|| anyhow::Error::msg("Invalid path string"))?
                .to_string(),
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
            let name =
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
                    })?;
            name
        }
    };
    println!("Verifying program: {}", library_name);

    let result = build_and_verify_repo(
        mount_path.to_str().unwrap().to_string(),
        base_image,
        bpf_flag,
        library_name,
        connection_url,
        program_id,
        workdir,
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

pub fn build_and_verify_repo(
    mount_path: String,
    base_image: Option<String>,
    bpf_flag: bool,
    library_name: String,
    connection_url: Option<String>,
    program_id: Pubkey,
    workdir: Option<String>,
    cargo_args: Vec<String>,
    container_id_opt: &mut Option<String>,
) -> anyhow::Result<(String, String)> {
    // Build the code using the docker container
    let executable_filename = format!("{}.so", &library_name);
    build(
        Some(mount_path.clone()),
        Some(library_name),
        base_image,
        bpf_flag,
        workdir,
        cargo_args,
        container_id_opt,
    )?;

    // Get the hash of the build
    let executable_path = std::process::Command::new("find")
        .args([
            &format!("{}/target/deploy", mount_path),
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
        .filter(|pkg| pkg.name.to_string() == package_name.to_string())
        .filter_map(|pkg| {
            let version = pkg.version.clone().to_string();
            let version_parts: Vec<&str> = version.split(".").collect();
            if version_parts.len() == 3 {
                let major = version_parts[0].parse::<u32>().unwrap_or(0);
                let minor = version_parts[1].parse::<u32>().unwrap_or(0);
                let patch = version_parts[2].parse::<u32>().unwrap_or(0);
                return Some((major, minor, patch));
            }
            return None;
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

pub fn get_rust_version_for_solana_version(
    major: u32,
    minor: u32,
    patch: u32,
) -> anyhow::Result<String> {
    let release = format!("v{}.{}.{}", major, minor, patch);
    if minor > 14 {
        // https://github.com/solana-labs/solana/commit/cdb204114ef529f9f63d4b5a995e0429919e3131
        // This is the first commit that moves the rust version to rust-toolchain.toml (1.15.0)
        let endpoint = format!(
            "https://raw.githubusercontent.com/solana-labs/solana/{}/rust-toolchain.toml",
            release
        );
        let body = reqwest::blocking::get(endpoint)?.text()?;
        body.split("\n")
            .skip(1)
            .next()
            .ok_or_else(|| anyhow!("Failed to parse rust version"))
            .map(|s| s.to_string().replace("channel = ", "").replace("\"", ""))
    } else {
        // For all previous releases, the rust version is in ci/rust-version.sh on line 21
        let endpoint = format!(
            "https://raw.githubusercontent.com/solana-labs/solana/{}/ci/rust-version.sh",
            release
        );
        let body = reqwest::blocking::get(endpoint)?.text()?;
        body.split("\n")
            .filter(|s| s.contains("stable_version"))
            .skip(1)
            .next()
            .ok_or_else(|| anyhow!("Failed to parse rust version"))
            .map(|s| {
                s.to_string()
                    .replace("stable_version=", "")
                    .replace(" ", "")
            })
    }
}

#[test]
fn test_rust_version() {
    for major in [1] {
        for minor in 10..18 {
            for patch in 0..30 {
                let res = get_rust_version_for_solana_version(major, minor, patch);
                if res.is_err() {
                    break;
                }
                println!("{}.{}.{}: {}", major, minor, patch, res.unwrap());
            }
        }
    }
}

#[test]
fn test_parse_cargo_log() {
    let res = get_pkg_version_from_cargo_lock("solana-program", "examples/hello_world/Cargo.lock")
        .unwrap();
    println!("{:?}", res);
    let (major, minor, patch) = res;
    println!(
        "{:?}",
        get_rust_version_for_solana_version(major, minor, patch).unwrap()
    );
}
