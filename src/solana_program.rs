use anyhow::anyhow;
use solana_cli_config::Config;
use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcProgramAccountsConfig,
    rpc_filter::{Memcmp, RpcFilterType},
};
use std::{
    io::{self, Read, Write},
    str::FromStr,
};

use borsh::{to_vec, BorshDeserialize, BorshSerialize};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, instruction::AccountMeta, message::Message, pubkey::Pubkey, signature::Keypair, signer::Signer, system_program, transaction::Transaction
};

use solana_account_decoder::UiAccountEncoding;
use solana_sdk::commitment_config::{CommitmentConfig, CommitmentLevel};

use crate::api::get_last_deployed_slot;

const OTTER_VERIFY_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("verifycLy8mB96wd9wqq3WDXQwM4oU6r42Th37Db9fC");
const OTTER_SIGNER: &str = "9VWiUUhgNoRwTH5NVehYJEDwcotwYX3VgW4MChiHPAqU";

#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct OtterBuildParams {
    pub address: Pubkey,
    pub signer: Pubkey,
    pub version: String,
    pub git_url: String,
    pub commit: String,
    pub args: Vec<String>,
    pub deployed_slot: u64,
    bump: u8,
}
impl std::fmt::Display for OtterBuildParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Program Id: {}", self.address)?;
        writeln!(f, "Signer: {}", self.signer)?;
        writeln!(f, "Git Url: {}", self.git_url)?;
        writeln!(f, "Commit: {}", self.commit)?;
        writeln!(f, "Deployed Slot: {}", self.deployed_slot)?;
        writeln!(f, "Args: {:?}", self.args)?;
        writeln!(f, "Version: {}", self.version)?;
        Ok(())
    }
}

pub fn prompt_user_input(message: &str) -> bool {
    let mut buffer = [0; 1];
    print!("{}", message);
    let _ = io::stdout().flush();
    io::stdin()
        .read_exact(&mut buffer)
        .expect("Unable to read user input");
    matches!(buffer[0] as char, 'Y' | 'y')
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct InputParams {
    pub version: String,
    pub git_url: String,
    pub commit: String,
    pub args: Vec<String>,
    pub deployed_slot: u64,
}

#[derive(PartialEq)]
pub enum OtterVerifyInstructions {
    Initialize,
    Update,
    Close,
}

impl OtterVerifyInstructions {
    fn get_discriminant(&self) -> Vec<u8> {
        match self {
            OtterVerifyInstructions::Initialize => vec![175, 175, 109, 31, 13, 152, 155, 237],
            OtterVerifyInstructions::Update => vec![219, 200, 88, 176, 158, 63, 253, 127],
            OtterVerifyInstructions::Close => vec![98, 165, 201, 177, 108, 65, 206, 96],
        }
    }
}

fn create_ix_data(params: &InputParams, ix: &OtterVerifyInstructions) -> Vec<u8> {
    let mut data = ix.get_discriminant(); // Discriminant for the instruction
    let params_data = to_vec(&params).expect("Unable to serialize params");
    data.extend_from_slice(&params_data);
    data
}

fn get_keypair_from_path(path: &str) -> anyhow::Result<Keypair> {
    solana_clap_utils::keypair::keypair_from_path(&Default::default(), &path, "keypair", false)
        .map_err(|err| anyhow!("Unable to get signer from path: {}", err))
}

fn get_user_config() -> anyhow::Result<(Keypair, RpcClient)> {
    let config_file = solana_cli_config::CONFIG_FILE
        .as_ref()
        .ok_or_else(|| anyhow!("Unable to get config file path"))?;
    let cli_config: Config = Config::load(config_file)?;

    let signer = get_keypair_from_path(&cli_config.keypair_path)?;

    let rpc_client = RpcClient::new(cli_config.json_rpc_url.clone());
    Ok((signer, rpc_client))
}

fn process_otter_verify_ixs(
    params: &InputParams,
    pda_account: Pubkey,
    program_address: Pubkey,
    instruction: OtterVerifyInstructions,
    rpc_client: &RpcClient,
    path_to_keypair: Option<String>,
    compute_unit_price: u64,
) -> anyhow::Result<()> {
    let user_config = get_user_config()?;
    let signer = if let Some(path_to_keypair) = path_to_keypair {
        get_keypair_from_path(&path_to_keypair)?
    } else {
        user_config.0
    };
    let signer_pubkey = signer.pubkey();
    let connection = rpc_client;

    let ix_data = if instruction != OtterVerifyInstructions::Close {
        create_ix_data(params, &instruction)
    } else {
        instruction.get_discriminant()
    };

    let mut accounts_meta_vec = vec![
        AccountMeta::new(pda_account, false),
        AccountMeta::new_readonly(signer_pubkey, true),
        AccountMeta::new_readonly(program_address, false),
    ];

    if instruction != OtterVerifyInstructions::Close {
        accounts_meta_vec.push(AccountMeta::new_readonly(system_program::ID, false));
    }

    let ix = solana_sdk::instruction::Instruction::new_with_bytes(
        OTTER_VERIFY_PROGRAM_ID,
        &ix_data,
        accounts_meta_vec,
    );

    let message = if compute_unit_price > 0 {
        // Add compute budget instruction for priority fees only if price > 0
        let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price);
        Message::new(&[compute_budget_ix, ix], Some(&signer_pubkey))
    } else {
        Message::new(&[ix], Some(&signer_pubkey))
    };

    let mut tx = Transaction::new_unsigned(message);

    tx.sign(&[&signer], connection.get_latest_blockhash()?);

    let tx_id = connection
        .send_and_confirm_transaction_with_spinner(&tx)
        .map_err(|err| {
            println!("{:?}", err);
            anyhow!("Failed to send transaction to the network.")
        })?;
    println!("Program uploaded successfully. Transaction ID: {}", tx_id);
    Ok(())
}

pub fn resolve_rpc_url(url: Option<String>) -> anyhow::Result<RpcClient> {
    let connection = match url.as_deref() {
        Some("m") => RpcClient::new("https://api.mainnet-beta.solana.com"),
        Some("d") => RpcClient::new("https://api.devnet.solana.com"),
        Some("t") => RpcClient::new("https://api.testnet.solana.com"),
        Some("l") => RpcClient::new("http://localhost:8899"),
        Some(url) => RpcClient::new(url),
        None => {
            if let Ok(cli_config) = get_user_config() {
                cli_config.1
            } else {
                RpcClient::new("https://api.mainnet-beta.solana.com")
            }
        }
    };

    Ok(connection)
}

pub async fn upload_program(
    git_url: String,
    commit: &Option<String>,
    args: Vec<String>,
    program_address: Pubkey,
    connection: &RpcClient,
    skip_prompt: bool,
    path_to_keypair: Option<String>,
    compute_unit_price: u64,
) -> anyhow::Result<()> {
    if skip_prompt
        || prompt_user_input(
            "Do you want to upload the program verification to the Solana Blockchain? (y/n) ",
        )
    {
        println!("Uploading the program verification params to the Solana blockchain...");

        let cli_config = get_user_config()?;

        let signer_pubkey: Pubkey = if let Some(ref path_to_keypair) = path_to_keypair {
            get_keypair_from_path(&path_to_keypair)?.pubkey()
        } else {
            cli_config.0.pubkey()
        };

        // let rpc_url = connection.url();
        println!("Using connection url: {}", connection.url());

        let last_deployed_slot = get_last_deployed_slot(&connection, &program_address.to_string())
            .await
            .map_err(|err| anyhow!("Unable to get last deployed slot: {}", err))?;

        let input_params = InputParams {
            version: env!("CARGO_PKG_VERSION").to_string(),
            git_url,
            commit: commit.clone().unwrap_or_default(),
            args,
            deployed_slot: last_deployed_slot,
        };

        // Possible PDA-1: Signer is current signer then we can update the program
        let pda_account_1 = find_build_params_pda(&program_address, &signer_pubkey).0;

        // Possible PDA-2: signer is otter signer
        let otter_signer = Pubkey::from_str(OTTER_SIGNER)?;
        let pda_account_2 = find_build_params_pda(&program_address, &otter_signer).0;

        if connection.get_account(&pda_account_1).is_ok() {
            println!("Program already uploaded by the current signer. Updating the program.");
            process_otter_verify_ixs(
                &input_params,
                pda_account_1,
                program_address,
                OtterVerifyInstructions::Update,
                connection,
                path_to_keypair,
                compute_unit_price,
            )?;
        } else if connection.get_account(&pda_account_2).is_ok() {
            let wanna_create_new_pda = skip_prompt || prompt_user_input(
                "Program already uploaded by another signer. Do you want to upload a new program? (Y/n)"
            );
            if wanna_create_new_pda {
                process_otter_verify_ixs(
                    &input_params,
                    pda_account_1,
                    program_address,
                    OtterVerifyInstructions::Initialize,
                    connection,
                    path_to_keypair,
                    compute_unit_price,
                )?;
            }
            return Ok(());
        } else {
            // Else Create new PDA and upload the program
            process_otter_verify_ixs(
                &input_params,
                pda_account_1,
                program_address,
                OtterVerifyInstructions::Initialize,
                connection,
                path_to_keypair,
                compute_unit_price,
            )?;
        }
    } else {
        println!("Exiting without uploading the program.");
    }

    Ok(())
}

fn find_build_params_pda(program_id: &Pubkey, signer: &Pubkey) -> (Pubkey, u8) {
    let seeds: &[&[u8]; 3] = &[b"otter_verify", &signer.to_bytes(), &program_id.to_bytes()];
    Pubkey::find_program_address(seeds, &OTTER_VERIFY_PROGRAM_ID)
}

pub async fn process_close(
    program_address: Pubkey,
    connection: &RpcClient,
    compute_unit_price: u64,
) -> anyhow::Result<()> {
    let user_config = get_user_config()?;
    let signer = user_config.0;
    let signer_pubkey = signer.pubkey();

    let last_deployed_slot = get_last_deployed_slot(&connection, &program_address.to_string())
        .await
        .map_err(|err| anyhow!("Unable to get last deployed slot: {}", err))?;

    let pda_account = find_build_params_pda(&program_address, &signer_pubkey).0;

    if connection.get_account(&pda_account).is_ok() {
        process_otter_verify_ixs(
            &InputParams {
                version: "".to_string(),
                git_url: "".to_string(),
                commit: "".to_string(),
                args: vec![],
                deployed_slot: last_deployed_slot,
            },
            pda_account,
            program_address,
            OtterVerifyInstructions::Close,
            connection,
            None,
            compute_unit_price,
        )?;
    } else {
        return Err(anyhow!(
            "No PDA found for signer {:?} and program address {:?}. Make sure you are providing the program address, not the PDA address. Check that a signer exists for the program by running `solana-verify list-program-pdas --program-id {:?}`",
            signer_pubkey,
            program_address,
            program_address
        ));
    }

    Ok(())
}

pub async fn get_program_pda(
    client: &RpcClient,
    program_id: &Pubkey,
    signer_pubkey: Option<String>,
) -> anyhow::Result<(Pubkey, OtterBuildParams)> {
    let signer_pubkey = if let Some(signer_pubkey) = signer_pubkey {
        Pubkey::from_str(&signer_pubkey)?
    } else {
        get_user_config()?.0.pubkey()
    };

    let pda = find_build_params_pda(program_id, &signer_pubkey).0;
    let account = client
        .get_account_with_commitment(
            &pda,
            CommitmentConfig {
                commitment: CommitmentLevel::Confirmed,
            },
        )
        .unwrap();
    if let Some(account) = account.value {
        Ok((
            pda,
            OtterBuildParams::try_from_slice(&account.data[8..])
                .map_err(|err| anyhow!("Unable to parse build params: {}", err))?,
        ))
    } else {
        Err(anyhow!(
            "PDA not found for {:?} and uploader {:?}. Make sure you've uploaded the PDA to mainnet.",
            program_id,
            signer_pubkey
        ))
    }
}

pub async fn get_all_pdas_available(
    client: &RpcClient,
    program_id_pubkey: &Pubkey,
) -> anyhow::Result<Vec<(Pubkey, OtterBuildParams)>> {
    let filter = vec![RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
        8,
        &program_id_pubkey.to_bytes(),
    ))];

    let config = RpcProgramAccountsConfig {
        filters: Some(filter),
        account_config: solana_client::rpc_config::RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            data_slice: None,
            commitment: Some(CommitmentConfig {
                commitment: CommitmentLevel::Confirmed,
            }),
            min_context_slot: None,
        },
        with_context: None,
    };

    let accounts = client.get_program_accounts_with_config(&OTTER_VERIFY_PROGRAM_ID, config)?;

    let mut pdas = vec![];
    for account in accounts {
        let otter_build_params = OtterBuildParams::try_from_slice(&account.1.data[8..]);
        if let Ok(otter_build_params) = otter_build_params {
            pdas.push((account.0, otter_build_params));
        }
    }

    Ok(pdas)
}
