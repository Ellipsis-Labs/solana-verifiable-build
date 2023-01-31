use std::{io::Read, path::Path};

use clap::{Parser, Subcommand};
use cmd_lib::{init_builtin_logger, run_cmd, run_fun};
use serde::Deserialize;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    pubkey::Pubkey,
};

#[derive(Parser, Debug)]
#[clap(author = "Ellipsis", version, about)]
struct Arguments {
    #[clap(subcommand)]
    subcommand: SubCommand,
}

#[derive(Deserialize, Debug)]
struct Config {
    package: Package,
}

#[derive(Deserialize, Debug)]
struct Package {
    name: String,
}

#[derive(Subcommand, Debug)]
enum SubCommand {
    /// Deterministically build the program in an Docker container
    Build {
        /// Path to mount to the docker image
        build_dir: Option<String>,
        #[clap(short, long)]
        base_image: Option<String>,
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
}

fn main() -> anyhow::Result<()> {
    let args = Arguments::parse();
    match args.subcommand {
        SubCommand::Build {
            build_dir: filepath,
            base_image,
        } => build(filepath, base_image),
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
            let client = RpcClient::new(url);
            let offset = UpgradeableLoaderState::size_of_buffer_metadata();
            let account_data = client.get_account_data(&buffer_address)?[offset..].to_vec();
            let program_hash = get_binary_hash(account_data);
            println!("{}", program_hash);
            Ok(())
        }
        SubCommand::GetProgramHash { url, program_id } => {
            let client = RpcClient::new(url);
            let program_buffer =
                Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id())
                    .0;
            let offset = UpgradeableLoaderState::size_of_programdata_metadata();
            let account_data = client.get_account_data(&program_buffer)?[offset..].to_vec();
            let program_hash = get_binary_hash(account_data);
            println!("{}", program_hash);
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
    let mut f = std::fs::File::open(&filepath)?;
    let metadata = std::fs::metadata(&filepath)?;
    let mut buffer = vec![0; metadata.len() as usize];
    f.read(&mut buffer)?;
    Ok(get_binary_hash(buffer))
}

pub fn build(filepath: Option<String>, base_image: Option<String>) -> anyhow::Result<()> {
    let path = filepath.unwrap_or(
        std::env::current_dir()?
            .as_os_str()
            .to_str()
            .ok_or(anyhow::Error::msg("Invalid path string"))?
            .to_string(),
    );
    println!("Mounting path: {}", path);
    let image = base_image.unwrap_or("ellipsislabs/solana:latest".to_string());
    init_builtin_logger();
    let container_id = run_fun!(
        docker run
        --rm
        -v $path:/build
        -dit $image
        sh -c "cargo build-sbf -- --locked --frozen"
    )?;
    run_cmd!(docker logs --follow $container_id)?;
    let build_path = Path::new(&path);
    let toml_path = build_path.join("Cargo.toml");
    let toml: Config = toml::from_str(&std::fs::read_to_string(&toml_path)?)?;
    let package_name = toml.package.name;
    let executable_path = Path::new(&path)
        .join("target")
        .join("deploy")
        .join(format!("{}.so", package_name));
    let program_hash = get_file_hash(executable_path.to_str().unwrap())?;
    println!("Executable hash: {}", program_hash);
    Ok(())
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
    println!("");
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
    println!("Executable hash (un-stripped): {}", executable_hash);
    println!("Program hash (un-stripped): {}", program_hash);

    if program_hash != executable_hash {
        println!("Executable hash mismatch");
        return Err(anyhow::Error::msg("Executable hash mismatch"));
    } else {
        println!("Executable matches on-chain program data ✅");
    }
    Ok(())
}
