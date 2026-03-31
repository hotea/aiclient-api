use anyhow::Result;
use aiclient_api::auth::{token_store::XdgTokenStore, TokenStore};

use super::commands::AuthAction;

pub async fn run(action: AuthAction) -> Result<()> {
    match action {
        AuthAction::Copilot { account_type: _ } => {
            let store = XdgTokenStore::default();
            aiclient_api::auth::copilot::authenticate(&store).await?;
            println!("Successfully authenticated with GitHub Copilot.");
        }
        AuthAction::Kiro { start_url, region } => {
            run_kiro_auth(start_url.as_deref(), region.as_deref()).await?;
        }
        AuthAction::List => {
            let store = XdgTokenStore::default();
            let providers = ["copilot", "kiro"];
            let mut found = false;
            for provider in &providers {
                match store.load(provider).await {
                    Ok(token_data) => {
                        found = true;
                        match token_data {
                            aiclient_api::auth::TokenData::Copilot {
                                github_token,
                                copilot_token,
                                expires_at,
                            } => {
                                println!("Provider: copilot");
                                println!(
                                    "  github_token: {}...{}",
                                    &github_token[..8.min(github_token.len())],
                                    if github_token.len() > 8 { "[redacted]" } else { "" }
                                );
                                if let Some(ct) = copilot_token {
                                    println!(
                                        "  copilot_token: {}...[redacted]",
                                        &ct[..8.min(ct.len())]
                                    );
                                }
                                if let Some(exp) = expires_at {
                                    println!("  expires_at: {}", exp);
                                }
                            }
                            aiclient_api::auth::TokenData::Kiro {
                                access_token,
                                region,
                                idc_region,
                                start_url,
                                auth_method,
                                expires_at,
                                ..
                            } => {
                                println!("Provider: kiro");
                                println!(
                                    "  access_token: {}...[redacted]",
                                    &access_token[..8.min(access_token.len())]
                                );
                                println!("  region: {}", region);
                                println!("  auth_method: {}", auth_method);
                                if let Some(idc) = idc_region {
                                    println!("  idc_region: {}", idc);
                                }
                                if let Some(url) = start_url {
                                    println!("  start_url: {}", url);
                                }
                                println!("  expires_at: {}", expires_at);
                            }
                        }
                    }
                    Err(_) => {
                        // Not authenticated for this provider, skip silently
                    }
                }
            }
            if !found {
                println!("No authenticated providers found.");
            }
        }
        AuthAction::Revoke { provider } => {
            let store = XdgTokenStore::default();
            store.delete(&provider).await?;
            println!("Revoked token for provider: {}", provider);
        }
    }
    Ok(())
}

async fn run_kiro_auth(start_url: Option<&str>, region: Option<&str>) -> Result<()> {
    // If --start-url is provided, go directly to org identity flow
    if let Some(url) = start_url {
        let region = region.unwrap_or("us-east-1");
        println!("Starting IAM Identity Center authentication...");
        println!("  Start URL: {}", url);
        println!("  Region: {}", region);
        let store = XdgTokenStore::default();
        aiclient_api::auth::kiro::authenticate_builder_id(&store, region, Some(url)).await?;
        println!("Successfully authenticated with Kiro (IAM Identity Center).");
        return Ok(());
    }

    println!("Select authentication method for Kiro:");
    println!("  1. AWS Builder ID (recommended)");
    println!("  2. Google account");
    println!("  3. GitHub account");
    println!("  4. Organization identity (IAM Identity Center)");
    print!("Enter choice (1-4): ");

    use std::io::Write;
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let choice = input.trim();

    let store = XdgTokenStore::default();
    let region = region.unwrap_or("us-east-1");

    match choice {
        "1" | "" => {
            println!("Starting AWS Builder ID authentication...");
            aiclient_api::auth::kiro::authenticate_builder_id(&store, region, None).await?;
            println!("Successfully authenticated with Kiro (Builder ID).");
        }
        "2" => {
            println!("Starting Google account authentication...");
            aiclient_api::auth::kiro::authenticate_social(&store, region, "google").await?;
            println!("Successfully authenticated with Kiro (Google).");
        }
        "3" => {
            println!("Starting GitHub account authentication...");
            aiclient_api::auth::kiro::authenticate_social(&store, region, "github").await?;
            println!("Successfully authenticated with Kiro (GitHub).");
        }
        "4" => {
            let (org_start_url, org_region) = prompt_org_identity(region)?;
            println!("Starting IAM Identity Center authentication...");
            println!("  Start URL: {}", org_start_url);
            println!("  Region: {}", org_region);
            aiclient_api::auth::kiro::authenticate_builder_id(
                &store,
                &org_region,
                Some(&org_start_url),
            )
            .await?;
            println!("Successfully authenticated with Kiro (IAM Identity Center).");
        }
        other => {
            anyhow::bail!("Invalid choice: {}. Please enter 1, 2, 3, or 4.", other);
        }
    }

    Ok(())
}

fn prompt_org_identity(default_region: &str) -> Result<(String, String)> {
    use std::io::Write;

    print!("Enter your IAM Identity Center Start URL: ");
    std::io::stdout().flush()?;
    let mut url_input = String::new();
    std::io::stdin().read_line(&mut url_input)?;
    let url = url_input.trim().to_string();
    if url.is_empty() {
        anyhow::bail!("Start URL cannot be empty.");
    }

    print!("Enter AWS region [{}]: ", default_region);
    std::io::stdout().flush()?;
    let mut region_input = String::new();
    std::io::stdin().read_line(&mut region_input)?;
    let region = region_input.trim();
    let region = if region.is_empty() {
        default_region.to_string()
    } else {
        region.to_string()
    };

    Ok((url, region))
}
