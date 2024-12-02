use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Success,
    Error,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyResponse {
    pub status: JobStatus,
    pub request_id: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub is_verified: bool,
    pub message: String,
    pub on_chain_hash: String,
    pub executable_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub status: Status,
    pub error: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JobResponse {
    pub status: JobStatus,
    pub respose: Option<JobVerificationResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum JobStatus {
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JobVerificationResponse {
    pub status: JobStatus,
    pub message: String,
    pub on_chain_hash: String,
    pub executable_hash: String,
    pub repo_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteStatusResponse {
    pub signer: String,
    pub is_verified: bool,
    pub on_chain_hash: String,
    pub executable_hash: String,
    pub repo_url: String,
    pub commit: String,
    pub last_verified_at: String,
}

impl std::fmt::Display for RemoteStatusResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Verification Status for Signer: {}", self.signer)?;
        writeln!(
            f,
            "Verified: {}",
            if self.is_verified { "✅" } else { "❌" }
        )?;
        writeln!(f, "On-chain Hash: {}", self.on_chain_hash)?;
        writeln!(f, "Executable Hash: {}", self.executable_hash)?;
        writeln!(f, "Repository URL: {}", self.repo_url)?;
        writeln!(f, "Commit: {}", self.commit)?;
        write!(f, "Last Verified: {}", self.last_verified_at)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteStatusResponseWrapper(Vec<RemoteStatusResponse>);

impl std::fmt::Display for RemoteStatusResponseWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, response) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
                writeln!(
                    f,
                    "----------------------------------------------------------------"
                )?;
            }
            write!(f, "{}", response)?;
        }
        Ok(())
    }
}
