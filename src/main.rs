use std::{io::Read, path::PathBuf};

use anyhow::anyhow;
use clap::{Parser, Subcommand};
use cmd_lib::{init_builtin_logger, run_cmd, run_fun};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    pubkey::Pubkey,
};
use uuid::Uuid;

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
        build_dir: Option<String>,
        #[clap(short, long)]
        base_image: Option<String>,
        /// If the program requires cargo build-bpf (instead of cargo build-sbf), as for anchor program, set this flag
        #[clap(short, long, default_value = "false")]
        bpf_flag: bool,
    },
    /// Verifies a cached build from a docker image
    VerifyFromImage {
        #[clap(short, long)]
        executable_path_in_image: String,
        #[clap(short, long)]
        image: String,
        #[clap(short, long, default_value = "https://api.mainnet-beta.solana.com")]
        url: String,
        #[clap(short, long)]
        program_id: Pubkey,
    },
    /// Get the hash of a program binary from an executable file
    GetExecutableHash {
        /// Path to the executable
        filepath: String,
    },
    /// Get the hash of a program binary from the deployed on-chain program
    GetProgramHash {
        #[clap(short, long, default_value = "https://api.mainnet-beta.solana.com")]
        url: String,
        /// Program ID
        program_id: Pubkey,
    },
    /// Get the hash of a program binary from the deployed buffer address
    GetBufferHash {
        #[clap(short, long, default_value = "https://api.mainnet-beta.solana.com")]
        url: String,
        /// Address of the buffer account containing the deployed program data
        buffer_address: Pubkey,
    },
    VerifyFromRepo {
        #[clap(short, long)]
        solana_program_path: String,
        repo_url: String,
        #[clap(short, long, default_value = "https://api.mainnet-beta.solana.com")]
        connection_url: String,
        #[clap(short, long)]
        program_id: Pubkey,
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
            build_dir: filepath,
            base_image,
            bpf_flag,
        } => {
            build(filepath, base_image, bpf_flag)?;
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
            program_id,
            connection_url,
            base_image,
            name_of_program,
            bpf_flag,
        } => {
            // Get source code from repo_url
            let base_name = run_fun!(basename $repo_url)?;
            let uuid = Uuid::new_v4().to_string();

            run_fun!(git clone $repo_url /tmp/solana-verify/$uuid/$base_name)?;

            // Get the absolute build path to the solana program directory to build inside docker
            let build_path = PathBuf::from(format!("/tmp/solana-verify/{}/{}", uuid, base_name))
                .join(solana_program_path.clone());
            println!("Build path: {:?}", build_path);

            // Build the code using the docker container
            build(
                Some(build_path.to_str().unwrap().to_string()),
                base_image,
                bpf_flag,
            )?;

            let executable_filename = format!("{}.so", name_of_program);

            // Get the hash of the build
            let executable_path = run_fun!(find /tmp/solana-verify/$uuid/$base_name/target/deploy -type f -name "$executable_filename")?;
            let build_hash = get_file_hash(&executable_path)?;

            // Get hash of on-chain program
            let program_hash = get_program_hash(connection_url, program_id)?;

            // Compare hashes
            println!("Executable Program Hash from repo: {}", build_hash);
            println!("On-chain Program Hash: {}", program_hash);

            // Remove temp repo
            run_fun!(rm -rf build_path)?;

            if program_hash != build_hash {
                println!("Executable hash mismatch");
                return Err(anyhow::Error::msg("Executable hash mismatch"));
            } else {
                println!("Executable matches on-chain program data ✅");
            }
            Ok(())
        }
    }
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

pub fn build(
    filepath: Option<String>,
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
        sh -c "$cargo_command -- --locked --frozen"
    )?;
    run_cmd!(docker logs --follow $container_id)?;
    Ok(())
}

pub fn get_buffer_hash(url: String, buffer_address: Pubkey) -> anyhow::Result<String> {
    let client = RpcClient::new(url);
    let offset = UpgradeableLoaderState::size_of_buffer_metadata();
    let account_data = client.get_account_data(&buffer_address)?[offset..].to_vec();
    let program_hash = get_binary_hash(account_data);
    Ok(program_hash)
}

pub fn get_program_hash(url: String, program_id: Pubkey) -> anyhow::Result<String> {
    let client = RpcClient::new(url);
    let program_buffer =
        Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id()).0;
    let offset = UpgradeableLoaderState::size_of_programdata_metadata();
    let account_data = client.get_account_data(&program_buffer)?[offset..].to_vec();
    let program_hash = get_binary_hash(account_data);
    Ok(program_hash)
}

pub fn verify_from_image(
    executable_path: String,
    image: String,
    network: String,
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
    let client = RpcClient::new(network);
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
