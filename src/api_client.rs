use anyhow::anyhow;
use console::Emoji;
use crossbeam_channel::{unbounded, Receiver};
use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
use reqwest::Client;
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use std::thread;
use std::time::{Duration, Instant};

// Emoji constants
static SPARKLE: Emoji<'_, '_> = Emoji("✨", ":-)");
static DONE: Emoji<'_, '_> = Emoji("✅", "");
static WAITING: Emoji<'_, '_> = Emoji("⏳", "");
static ERROR: Emoji<'_, '_> = Emoji("❌", "X");

// URL for the remote server
pub const REMOTE_SERVER_URL: &str = "https://verify.osec.io/verify_async";

fn loading_animation(receiver: Receiver<bool>) {
    let started = Instant::now();
    let spinner_style =
        ProgressStyle::with_template("[{elapsed_precise}] {prefix:.bold.dim} {spinner} {wide_msg}")
            .unwrap()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");

    let pb = ProgressBar::new_spinner();
    pb.set_style(spinner_style);
    pb.set_message(format!(
        "Request sent. Awaiting server response. This may take a moment... {}",
        WAITING
    ));
    loop {
        match receiver.try_recv() {
            Ok(result) => {
                if result {
                    pb.finish_with_message(format!("{} Process completed.", DONE));
                    println!("{} Done in {}", SPARKLE, HumanDuration(started.elapsed()));
                } else {
                    pb.finish_with_message(format!("{} Request processing failed.", ERROR));
                    println!(
                        "{} Time elapsed : {}",
                        ERROR,
                        HumanDuration(started.elapsed())
                    );
                }
                break;
            }

            Err(_) => {
                pb.inc(1);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

// Send a job to the remote server
#[allow(clippy::too_many_arguments)]
pub async fn send_job_to_remote(
    repo_url: &str,
    commit_hash: &Option<String>,
    program_id: &Pubkey,
    library_name: &Option<String>,
    bpf_flag: bool,
    relative_mount_path: String,
    base_image: Option<String>,
    cargo_args: Vec<String>,
) -> anyhow::Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(1800))
        .build()?;

    // Create a channel for communication between threads
    let (sender, receiver) = unbounded();

    let handle = thread::spawn(move || loading_animation(receiver));
    // Send the POST request
    let response = client
        .post(REMOTE_SERVER_URL)
        .json(&json!({
            "repository": repo_url,
            "commit_hash": commit_hash,
            "program_id": program_id.to_string(),
            "lib_name": library_name,
            "bpf_flag": bpf_flag,
            "mount_path":  if relative_mount_path.is_empty() {
                None
            } else {
                Some(relative_mount_path)
            },
            "base_image": base_image,
            "cargo_args": cargo_args,
        }))
        .send()
        .await?;

    if response.status().is_success() || response.status() == 409 {
        sender.send(true)?;
        handle.join().unwrap();
        let status_response: Value = serde_json::from_str(&response.text().await?)?;

        if let Some(is_verified) = status_response["is_verified"].as_bool() {
            if is_verified {
                println!("Program {} has already been verified. {}", program_id, DONE);
                println!(
                    "On Chain Hash: {}",
                    status_response["on_chain_hash"].as_str().unwrap_or("")
                );
                println!(
                    "Executable Hash: {}",
                    status_response["executable_hash"].as_str().unwrap_or("")
                );
            } else {
                println!("We have already processed this request.");
                println!("Program {} has not been verified. {}", program_id, ERROR);
            }
        } else if status_response["status"] == "error" {
            println!("Error encountered while processing request.");
            println!(
                "Error message: {}",
                status_response["error"].as_str().unwrap_or("")
            );
        } else {
            println!("We have already processed this request.");
        }

        Ok(())
    } else {
        sender.send(false)?;
        handle.join().unwrap();
        Err(anyhow!(
            "Encountered an error while attempting to send the job to remote : {:?}",
            response.text().await?
        ))?
    }
}
