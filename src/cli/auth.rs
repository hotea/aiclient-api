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
            println!("Kiro auth not yet implemented");
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
                                expires_at,
                                ..
                            } => {
                                println!("Provider: kiro");
                                println!(
                                    "  access_token: {}...[redacted]",
                                    &access_token[..8.min(access_token.len())]
                                );
                                println!("  region: {}", region);
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
