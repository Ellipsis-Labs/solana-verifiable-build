use clap::{Parser, Subcommand};
use cmd_lib::{init_builtin_logger, run_cmd, run_fun};
use sha1::{Digest, Sha1};
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

#[derive(Subcommand, Debug)]
enum SubCommand {
    Build {
        #[clap(short, long)]
        filepath: Option<String>,
        #[clap(short, long)]
        base_image: Option<String>,
    },
    Verify {
        #[clap(short, long)]
        executable_path: String,
        #[clap(short, long)]
        image: String,
        #[clap(short, long, default_value = "https://api.mainnet-beta.solana.com")]
        url: String,
        #[clap(short, long)]
        program_id: Pubkey,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Arguments::parse();
    match args.subcommand {
        SubCommand::Build {
            filepath,
            base_image,
        } => {
            let path = filepath.unwrap_or("$(pwd)".to_string());
            let image = base_image.unwrap_or("ellipsislabs/solana:latest".to_string());
            init_builtin_logger();
            let container_id = run_fun!(
                docker run
                --rm
                -v $path:/work
                -dit $image
                sh -c "cargo build-sbf -- --locked --frozen"
            )?;
            run_cmd!(docker logs --follow $container_id)?;
        }
        SubCommand::Verify {
            executable_path,
            image,
            url: network,
            program_id,
        } => {
            println!(
                "Verifying image: {:?}, on network {:?} against program ID {}",
                image, network, program_id
            );
            let output = run_fun!(
                docker run --rm
                -it $image  sh -c
                "(wc -c $executable_path && shasum $executable_path) | tr '\n' ' '"
                | tail -n 1
                | awk "{print $1, $3}"
            )?;
            let tokens = output.split_whitespace().collect::<Vec<_>>();
            let executable_size = tokens[0].parse::<usize>()?;
            let executable_hash = tokens[1];
            let client = RpcClient::new(network);
            let program_buffer =
                Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id())
                    .0;

            let offset = UpgradeableLoaderState::size_of_programdata_metadata();
            let account_data =
                &client.get_account_data(&program_buffer)?[offset..offset + executable_size];
            let mut hasher = Sha1::new();
            hasher.update(account_data);
            let program_hash = hasher.finalize();
            if hex::encode(program_hash) != executable_hash {
                println!("Executable hash mismatch");
                return Err(anyhow::Error::msg("Executable hash mismatch"));
            } else {
                println!("Executable matches on-chain program data ✅");
            }
        }
    }
    Ok(())
}
