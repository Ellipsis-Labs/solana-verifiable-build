use clap::{Parser, Subcommand};

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
        keypair: Option<String>,
        #[clap(short, long)]
        network: String,
    },
    Verify {
        #[clap(short, long)]
        github_url: String,
        #[clap(short, long)]
        network: String,
    },
}

fn main() {
    let args = Arguments::parse();
    match args.subcommand {
        SubCommand::Build { keypair, network } => {
            println!(
                "Building with keypair: {:?}, on network {:?}",
                keypair, network
            );
        }
        SubCommand::Verify {
            github_url,
            network,
        } => {
            println!(
                "Verifying with github url: {:?}, on network {:?}",
                github_url, network
            );
        }
    }
}
