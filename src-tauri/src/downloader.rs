use anyhow::Result;
use reqwest::Client;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::fs::File;
use sha2::{Sha256, Digest};

#[allow(dead_code)]
pub struct Downloader;

impl Downloader {
    #[allow(dead_code)]
    pub async fn download_model(url: &str, dest_path: &Path, expected_sha256: Option<&str>) -> Result<()> {
        let client = Client::new();
        let mut response = client.get(url).send().await?.error_for_status()?;
        
        let mut file = File::create(dest_path).await?;
        let mut hasher = Sha256::new();
        
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
            hasher.update(&chunk);
        }
        
        file.sync_all().await?;
        
        if let Some(expected) = expected_sha256 {
            let hash = format!("{:x}", hasher.finalize());
            if hash != expected {
                return Err(anyhow::anyhow!("SHA256 mismatch: expected {expected}, got {hash}"));
            }
        }
        
        Ok(())
    }
}
