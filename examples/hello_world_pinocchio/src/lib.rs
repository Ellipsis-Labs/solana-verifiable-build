use pinocchio::{program_entrypoint, AccountView, Address, ProgramResult};
use solana_program_log::log;

program_entrypoint!(process_instruction);

fn process_instruction(
    _program_id: &Address,
    _accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    log("Hello, world!");
    Ok(())
}
