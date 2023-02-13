use std::{io::Read, path::PathBuf};

use anyhow::anyhow;
use clap::{Parser, Subcommand};
use cmd_lib::{init_builtin_logger, run_cmd, run_fun};
use solana_cli_config::{Config, CONFIG_FILE};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    pubkey::Pubkey,
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
}

#[derive(Subcommand, Debug)]
enum SubCommand {
    /// Deterministically build the program in an Docker container
    Build {
        /// Path to mount to the docker image
        mount_dir: Option<String>,
        /// Path to build the docker image
        #[clap(short, long)]
        program_build_dir: Option<String>,
        /// Optionally specify a custom base docker image to use for building the program repository
        #[clap(short, long)]
        base_image: Option<String>,
        /// If the program requires cargo build-bpf (instead of cargo build-sbf), as for anchor program, set this flag
        #[clap(long, default_value = "false")]
        bpf_flag: bool,
    },
    /// Verifies a cached build from a docker image
    VerifyFromImage {
        /// Path to the executable solana program within the source code repository in the docker image
        #[clap(short, long)]
        executable_path_in_image: String,
        /// Image that contains the source code to be verified
        #[clap(short, long)]
        image: String,
        /// Connection URL to Solana network to verify the on-chain program. Defaults to user global config
        #[clap(short, long)]
        url: Option<String>,
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
        /// Connection URL to Solana network to verify the on-chain program. Defaults to user global config
        #[clap(short, long)]
        url: Option<String>,
        /// The Program ID of the program to verify
        program_id: Pubkey,
    },
    /// Get the hash of a program binary from the deployed buffer address
    GetBufferHash {
        /// Connection URL to Solana network to verify the on-chain program. Defaults to user global config
        #[clap(short, long)]
        url: Option<String>,
        /// Address of the buffer account containing the deployed program data
        buffer_address: Pubkey,
    },
    /// Builds and verifies a program from a given repository URL and a program ID
    VerifyFromRepo {
        /// Path to the executable solana program within the source code repository if the program is not part of the top-level Cargo.toml
        #[clap(short, long, default_value = ".")]
        solana_program_path: String,
        /// The HTTPS URL of the repo to clone
        repo_url: String,
        /// Optional commit hash to checkout
        #[clap(long)]
        commit_hash: Option<String>,
        /// Connection URL to Solana network to verify the on-chain program. Defaults to user global config
        #[clap(short, long)]
        url_solana: Option<String>,
        /// The Program ID of the program to verify
        #[clap(short, long)]
        program_id: Pubkey,
        /// Optionally specify a custom base docker image to use for building the program repository
        #[clap(short, long)]
        base_image: Option<String>,
        /// If the repo_url points to a repo that contains multiple programs, specify the name of the program to build and verify
        #[clap(short, long, default_value = "*")]
        name_of_program: String,
        /// If the program requires cargo build-bpf (instead of cargo build-sbf), as for anchor program, set this flag
        #[clap(long, default_value = "false")]
        bpf_flag: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Arguments::parse();
    match args.subcommand {
        SubCommand::Build {
            mount_dir: filepath,
            program_build_dir,
            base_image,
            bpf_flag,
        } => {
            build(filepath, program_build_dir, base_image, bpf_flag)?;
            Ok(())
        }
        SubCommand::VerifyFromImage {
            executable_path_in_image: executable_path,
            image,
            url: network,
            program_id,
        } => verify_from_image(executable_path, image, network, program_id),
        SubCommand::GetExecutableHash { filepath } => {
            let program_hash = get_file_hash(&filepath)?;
            println!("{}", program_hash);
            Ok(())
        }
        SubCommand::GetBufferHash {
            url,
            buffer_address,
        } => {
            let buffer_hash = get_buffer_hash(url, buffer_address)?;
            println!("{}", buffer_hash);
            Ok(())
        }
        SubCommand::GetProgramHash { url, program_id } => {
            let program_hash = get_program_hash(url, program_id)?;
            println!("{}", program_hash);
            Ok(())
        }
        SubCommand::VerifyFromRepo {
            solana_program_path,
            repo_url,
            commit_hash,
            program_id,
            url_solana,
            base_image,
            name_of_program,
            bpf_flag,
        } => {
            // Get source code from repo_url
            let base_name = run_fun!(basename $repo_url)?;
            let uuid = Uuid::new_v4().to_string();

            // Create a temporary directory to clone the repo into
            let tmp_file_path = format!("/tmp/solana-verify/{}/{}", uuid, base_name);
            run_fun!(git clone $repo_url $tmp_file_path)?;

            // Checkout a specific commit hash, if provided
            if let Some(commit_hash) = commit_hash {
                println!("tmp_file_path: {:?}", tmp_file_path);
                let result = run_fun!(cd $tmp_file_path; git checkout $commit_hash);
                if result.is_ok() {
                    println!("Checked out commit hash: {}", commit_hash);
                } else {
                    run_fun!(rm -rf /tmp/solana-verify/$uuid)?;
                    Err(anyhow!("Failed to checkout commit hash: {:?}", result))?;
                }
            }

            // Get the absolute build path to the solana program directory to build inside docker
            let build_path = PathBuf::from(tmp_file_path.clone()).join(solana_program_path);
            println!("Build path: {:?}", build_path);

            let result = verify_from_repo(
                build_path.to_str().unwrap().to_string(),
                base_image,
                bpf_flag,
                name_of_program,
                url_solana,
                program_id,
            );

            // Cleanup no matter the result
            run_fun!(rm -rf /tmp/solana-verify/$uuid)?;

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
    }
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
    filepath: Option<String>,
    buildpath: Option<String>,
    base_image: Option<String>,
    bpf_flag: bool,
) -> anyhow::Result<()> {
    let path = filepath.unwrap_or(
        std::env::current_dir()?
            .as_os_str()
            .to_str()
            .ok_or_else(|| anyhow::Error::msg("Invalid path string"))?
            .to_string(),
    );
    println!("Mounting path: {}", path);
    let image = base_image.unwrap_or_else(|| "ellipsislabs/solana:latest".to_string());
    let cargo_command = if bpf_flag {
        "cargo build-bpf"
    } else {
        "cargo build-sbf"
    };

    let cd_dir = if buildpath.is_none() {
        format!("cd .")
    } else {
        format!("cd {}", buildpath.unwrap())
    };

    println!(
        "Cargo build command: {} -- --locked --frozen",
        cargo_command
    );

    init_builtin_logger();
    let container_id = run_fun!(
        docker run
        --rm
        -v $path:/build
        -dit $image
        sh -c "$cd_dir && $cargo_command -- --locked --frozen"
    )?;
    run_cmd!(docker logs --follow $container_id)?;
    Ok(())
}

pub fn verify_from_image(
    executable_path: String,
    image: String,
    network: Option<String>,
    program_id: Pubkey,
) -> anyhow::Result<()> {
    println!(
        "Verifying image: {:?}, on network {:?} against program ID {}",
        image, network, program_id
    );
    println!("Executable path in container: {:?}", executable_path);
    println!(" ");
    let container_id = run_fun!(
        docker run --rm -dit $image
    )?;
    run_cmd!(docker cp $container_id:/build/$executable_path /tmp/program.so)?;

    let executable_hash = get_file_hash("/tmp/program.so")?;
    let client = get_client(network);
    let program_buffer =
        Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id()).0;
    let offset = UpgradeableLoaderState::size_of_programdata_metadata();
    let account_data = &client.get_account_data(&program_buffer)?[offset..];
    let program_hash = get_binary_hash(account_data.to_vec());
    println!("Executable hash: {}", executable_hash);
    println!("Program hash: {}", program_hash);

    // Cleanup docker and rm file
    run_fun!(docker kill $container_id)?;
    run_fun!(rm "/tmp/program.so")?;

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
    name_of_program: String,
    connection_url: Option<String>,
    program_id: Pubkey,
) -> anyhow::Result<(String, String)> {
    // Build the code using the docker container
    build(Some(base_repo_path.clone()), None, base_image, bpf_flag)?;

    let executable_filename = format!("{}.so", name_of_program);

    // Get the hash of the build
    println!(
        "Looking for executable name {} at path: {}/target/deploy",
        executable_filename, base_repo_path
    );
    let executable_path =
        run_fun!(find $base_repo_path/target/deploy -name "$executable_filename")?;
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
