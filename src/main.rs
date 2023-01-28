use clap::{Parser, Subcommand};

#[macro_use]
extern crate rust_shell;
use std::{io::Read, process::Stdio};

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
    },
    Verify {
        #[clap(short, long)]
        github_url: String,
        #[clap(short, long)]
        network: String,
        #[clap(short, long)]
        program_id: String,
    },
}

fn main() {
    let args = Arguments::parse();
    match args.subcommand {
        SubCommand::Build { filepath } => {
            println!("Building from path: {:?}", filepath);
            let path = filepath.unwrap_or("$(pwd)".to_string());

            let mut run = cmd!(
                "docker run --rm -v {}:/work -dit ellipsislabs/solana:1.14.13 sh -c \"cargo build-sbf -- --locked --frozen 2>&1\"",
                &path
            );

            {
                let command = &mut run.command;
                command.stdout(Stdio::piped());
            }

            // Access std::process::Child.
            let mut container_id = String::new();
            let shell_child = run.spawn().unwrap();
            {
                let mut lock = shell_child.0.write().unwrap();
                let child = &mut lock.as_mut().unwrap().child;
                child
                    .stdout
                    .as_mut()
                    .unwrap()
                    .read_to_string(&mut container_id)
                    .unwrap();
            }
            println!("waiting for container: {}", &container_id);

            let wait = cmd!("docker logs --follow {}", &container_id.trim_end());
            // Access std::process::Child.
            wait.run().unwrap();
        }
        SubCommand::Verify {
            github_url,
            network,
            program_id,
        } => {
            println!(
                "Verifying with github url: {:?}, on network {:?}",
                github_url, network
            );
        }
    }
}
