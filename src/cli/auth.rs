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
        AuthAction::Kiro => {
            run_kiro_auth().await?;
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

async fn run_kiro_auth() -> Result<()> {
    println!("Select authentication method for Kiro:");
    println!("  1. AWS Builder ID (recommended)");
    println!("  2. Google account");
    println!("  3. GitHub account");
    print!("Enter choice (1-3): ");

    use std::io::Write;
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let choice = input.trim();

    let store = XdgTokenStore::default();
    let region = "us-east-1";

    match choice {
        "1" | "" => {
            println!("Starting AWS Builder ID authentication...");
            aiclient_api::auth::kiro::authenticate_builder_id(&store, region).await?;
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
        other => {
            anyhow::bail!("Invalid choice: {}. Please enter 1, 2, or 3.", other);
        }
    }

    Ok(())
}
