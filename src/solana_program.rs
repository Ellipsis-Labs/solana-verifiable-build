use anyhow::anyhow;
use solana_cli_config::Config;
use solana_client::rpc_client::RpcClient;
use std::{
    io::{self, Read, Write},
    str::FromStr,
};

use borsh::{to_vec, BorshDeserialize, BorshSerialize};
use solana_sdk::{
    instruction::AccountMeta, message::Message, pubkey::Pubkey, signer::Signer, system_program,
    transaction::Transaction,
};

const OTTER_VERIFY_PROGRAMID: &str = "EngB3ANqXh8nDFhzZYJkCfpCHWCHkTrJTCWKEuSFCh7B";

pub fn prompt_user_input() -> bool {
    let mut buffer = [0; 1];
    print!("Do you want to update it to On-Chain Program ?. (Y/n) ");
    // Read a single character from standard input
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
}

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
fn create_ix_data(params: &InputParams, ix: OtterVerifyInstructions) -> Vec<u8> {
    let mut data = ix.get_discriminant(); // Discriminant for the instruction
    let params_data = to_vec(&params).expect("Unable to serialize params");
    data.extend_from_slice(&params_data);
    data
}

pub async fn upload_program(
    git_url: String,
    commit: &Option<String>,
    args: Vec<String>,
    other_program_address: Pubkey,
) -> anyhow::Result<()> {
    if true {
        println!("Uploading the program to the Solana blockchain...");

        let config_file = solana_cli_config::CONFIG_FILE
            .as_ref()
            .ok_or_else(|| anyhow!("unable to get config file path"))?;
        let cli_config: Config = Config::load(config_file)?;

        let connection = RpcClient::new(cli_config.json_rpc_url);

        let signer = solana_clap_utils::keypair::keypair_from_path(
            &Default::default(),
            &cli_config.keypair_path,
            "keypair",
            false,
        )
        .map_err(|err| anyhow!("Unable to get signer from path: {}", err))?;
        let signer_pubkey = signer.pubkey();

        let program_id = Pubkey::from_str(OTTER_VERIFY_PROGRAMID).unwrap();

        let seeds: &[&[u8]; 3] = &[
            b"otter_verify",
            &signer_pubkey.to_bytes(),
            &other_program_address.to_bytes(),
        ];

        let (pda_account, _) = Pubkey::find_program_address(seeds, &program_id);

        // TODO: Need to get Version from Cargo.toml
        let input_params = InputParams {
            version: "0.2.11".to_string(),
            git_url,
            commit: commit.clone().unwrap_or_default(),
            args,
        };

        let ix_data = create_ix_data(&input_params, OtterVerifyInstructions::Initialize);

        let ix = solana_sdk::instruction::Instruction::new_with_bytes(
            program_id,
            &ix_data,
            vec![
                AccountMeta::new(pda_account, false),
                AccountMeta::new_readonly(signer_pubkey, true),
                AccountMeta::new_readonly(other_program_address, false),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
        );
        let message = Message::new(&[ix], Some(&signer_pubkey));

        let mut tx = Transaction::new_unsigned(message);

        tx.sign(&[&signer], connection.get_latest_blockhash()?);

        let tx_id = connection
            .send_and_confirm_transaction_with_spinner(&tx)
            .map_err(|err| {
                println!("{:?}", err);
                anyhow!("Failed to send transaction to the network.",)
            })?;
        println!("Program uploaded successfully. Transaction ID: {}", tx_id);
    } else {
        println!("Exiting without uploading the program.");
    }

    Ok(())
}
