use anyhow::anyhow;
use crossbeam_channel::{unbounded, Receiver};
use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
use reqwest::{Client, Response};
use serde_json::json;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use crate::api::models::{
    ErrorResponse, JobResponse, JobStatus, JobVerificationResponse, RemoteStatusResponseWrapper,
    VerifyResponse,
};
use crate::solana_program::get_program_pda;
use crate::SIGNAL_RECEIVED;
use crate::{get_genesis_hash, MAINNET_GENESIS_HASH};

// URL for the remote server
pub const REMOTE_SERVER_URL: &str = "https://verify.osec.io";

fn loading_animation(receiver: Receiver<bool>) {
    let started = Instant::now();
    let spinner_style =
        ProgressStyle::with_template("[{elapsed_precise}] {prefix:.bold.dim} {spinner} {wide_msg}")
            .unwrap()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");

    let pb = ProgressBar::new_spinner();
    pb.set_style(spinner_style);
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_message("Request sent. Awaiting server response. This may take a moment... ⏳");

    loop {
        // Check if interrupt signal was received
        if SIGNAL_RECEIVED.load(Ordering::Relaxed) {
            pb.finish_with_message("❌ Operation interrupted by user.");
            break;
        }

        match receiver.try_recv() {
            Ok(result) => {
                if result {
                    pb.finish_with_message(format!(
                        "✅ Process completed. (Done in {})\n",
                        HumanDuration(started.elapsed())
                    ));
                } else {
                    pb.finish_with_message("❌ Request processing failed.");
                    println!("❌ Time elapsed : {}", HumanDuration(started.elapsed()));
                }
                break;
            }
            Err(_) => {
                if SIGNAL_RECEIVED.load(Ordering::Relaxed) {
                    pb.finish_with_message("❌ Operation interrupted by user.");
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
    pb.abandon(); // Ensure the progress bar is cleaned up
}

fn print_verification_status(
    program_id: &str,
    status: bool,
    status_response: &JobVerificationResponse,
) {
    let status_message = if status {
        format!("Program {} has been verified. ✅", program_id)
    } else {
        format!("Program {} has not been verified. ❌", program_id)
    };
    let message = if status {
        "The provided GitHub build matches the on-chain hash."
    } else {
        "The provided GitHub build does not match the on-chain hash."
    };
    println!("{}", status_message);
    println!("{}", message);
    println!("On Chain Hash: {}", status_response.on_chain_hash.as_str());
    println!(
        "Executable Hash: {}",
        status_response.executable_hash.as_str()
    );
    println!("Repo URL: {}", status_response.repo_url.as_str());
}

pub async fn send_job_with_uploader_to_remote(
    connection: &RpcClient,
    program_id: &Pubkey,
    uploader: &Pubkey,
) -> anyhow::Result<()> {
    // Check that PDA exists before sending job
    let genesis_hash = get_genesis_hash(connection)?;
    if genesis_hash != MAINNET_GENESIS_HASH {
        return Err(anyhow!("Remote verification only works with mainnet. Please omit the --remote flag to verify locally."));
    }
    get_program_pda(connection, program_id, Some(uploader.to_string())).await?;

    let client = Client::builder()
        .timeout(Duration::from_secs(18000))
        .build()?;

    // Send the POST request
    let response = client
        .post(format!("{}/verify-with-signer", REMOTE_SERVER_URL))
        .json(&json!({
            "program_id": program_id.to_string(),
            "signer": uploader.to_string(),
            "repository": "",
            "commit_hash": "",
        }))
        .send()
        .await?;

    handle_submission_response(&client, response, program_id).await
}

pub async fn handle_submission_response(
    client: &Client,
    response: Response,
    program_id: &Pubkey,
) -> anyhow::Result<()> {
    if response.status().is_success() {
        // First get the raw text to preserve it in case of parsing failure
        let response_text = response.text().await?;
        let status_response =
            serde_json::from_str::<VerifyResponse>(&response_text).map_err(|e| {
                eprintln!("Failed to parse response as VerifyResponse: {}", e);
                eprintln!("Raw response: {}", response_text);
                anyhow!("Failed to parse server response")
            })?;
        let request_id = status_response.request_id;
        println!("Verification request sent with request id: {}", request_id);
        println!("Verification in progress... ⏳");

        // Span new thread for polling the server for status
        // Create a channel for communication between threads
        let (sender, receiver) = unbounded();
        let handle = thread::spawn(move || loading_animation(receiver));

        loop {
            // Check for interrupt signal before polling
            if SIGNAL_RECEIVED.load(Ordering::Relaxed) {
                let _ = sender.send(false);
                handle.join().unwrap();
                break; // Exit the loop and continue with normal error handling
            }

            let status = check_job_status(client, &request_id).await?;
            match status.status {
                JobStatus::InProgress => {
                    if SIGNAL_RECEIVED.load(Ordering::Relaxed) {
                        let _ = sender.send(false);
                        handle.join().unwrap();
                        break;
                    }
                    thread::sleep(Duration::from_secs(10));
                }
                JobStatus::Completed => {
                    let _ = sender.send(true);
                    handle.join().unwrap();
                    let status_response = status.respose.unwrap();

                    if status_response.executable_hash == status_response.on_chain_hash {
                        print_verification_status(
                            program_id.to_string().as_str(),
                            true,
                            &status_response,
                        );
                    } else {
                        print_verification_status(
                            program_id.to_string().as_str(),
                            false,
                            &status_response,
                        );
                    }
                    break;
                }
                JobStatus::Failed => {
                    let _ = sender.send(false);
                    handle.join().unwrap();
                    let status_response: JobVerificationResponse = status.respose.unwrap();
                    println!("Program {} has not been verified. ❌", program_id);
                    eprintln!("Error message: {}", status_response.message.as_str());
                    println!(
                        "You can check the logs for more details here: {}/logs/{}",
                        REMOTE_SERVER_URL, request_id
                    );
                    break;
                }
                JobStatus::Unknown => {
                    let _ = sender.send(false);
                    handle.join().unwrap();
                    println!("Program {} has not been verified. ❌", program_id);
                    break;
                }
            }
        }
        let url = format!("https://verify.osec.io/status/{}", program_id);
        println!("Check the verification status at: {}", url);
        println!(
            "Job url: {}",
            &format!("{}/job/{}", REMOTE_SERVER_URL, request_id)
        );

        Ok(())
    } else if response.status() == 409 {
        let response = response.json::<ErrorResponse>().await?;
        eprintln!("Error: {}", response.error.as_str());
        let url = format!("{}/status/{}", REMOTE_SERVER_URL, program_id);
        println!("Check the status at: {}", url);
        Ok(())
    } else {
        eprintln!("Encountered an error while attempting to send the job to remote");
        Err(anyhow!("{:?}", response.text().await?))?;
        let url = format!("{}/status/{}", REMOTE_SERVER_URL, program_id);
        println!("Check the verification status at: {}", url);
        Ok(())
    }
}

async fn check_job_status(client: &Client, request_id: &str) -> anyhow::Result<JobResponse> {
    // Get /job/:id
    let response = client
        .get(format!("{}/job/{}", REMOTE_SERVER_URL, request_id))
        .send()
        .await
        .unwrap();

    if response.status().is_success() {
        // Parse the response
        let response: JobVerificationResponse = response.json().await?;
        match response.status {
            JobStatus::InProgress => {
                thread::sleep(Duration::from_secs(5));
                Ok(JobResponse {
                    status: JobStatus::InProgress,
                    respose: None,
                })
            }
            JobStatus::Completed => Ok(JobResponse {
                status: JobStatus::Completed,
                respose: Some(response),
            }),
            JobStatus::Failed => Ok(JobResponse {
                status: JobStatus::Failed,
                respose: Some(response),
            }),
            JobStatus::Unknown => Ok(JobResponse {
                status: JobStatus::Unknown,
                respose: Some(response),
            }),
        }
    } else {
        Err(anyhow!(
            "Encountered an error while attempting to check job status : {:?}",
            response.text().await?
        ))?
    }
}

pub async fn get_remote_status(program_id: Pubkey) -> anyhow::Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(18000))
        .build()?;

    let response = client
        .get(format!("{}/status-all/{}", REMOTE_SERVER_URL, program_id,))
        .send()
        .await?;

    let status: RemoteStatusResponseWrapper = response.json().await?;
    println!("{}", status);
    Ok(())
}

pub async fn get_remote_job(job_id: &str) -> anyhow::Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(18000))
        .build()?;

    let job = check_job_status(&client, job_id).await?;
    println!("{}", job);
    Ok(())
}
