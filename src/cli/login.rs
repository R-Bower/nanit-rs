use crate::api::client::NanitClient;
use crate::api::error::NanitError;
use crate::session::init_session_store;
use crate::util::prompt_input;

pub async fn run(session_path: &str) -> anyhow::Result<()> {
    let mut session = init_session_store(session_path);
    let client = NanitClient::new();

    let email = prompt_input("Email: ")?;
    let password = prompt_input("Password: ")?;

    match client.login(&mut session, &email, &password).await {
        Ok(result) => {
            println!("Login successful!");
            println!("Access token: {}...", &result.access_token[..8.min(result.access_token.len())]);
            println!("Refresh token: {}...", &result.refresh_token[..8.min(result.refresh_token.len())]);
        }
        Err(NanitError::MfaRequired {
            mfa_token,
            phone_suffix,
            ..
        }) => {
            println!("MFA required (phone: ...{phone_suffix})");
            let mfa_code = prompt_input("MFA code: ")?;

            let result = client
                .login_with_mfa(&mut session, &email, &password, &mfa_token, &mfa_code)
                .await?;
            println!("Login successful!");
            println!("Access token: {}...", &result.access_token[..8.min(result.access_token.len())]);
            println!("Refresh token: {}...", &result.refresh_token[..8.min(result.refresh_token.len())]);
        }
        Err(e) => return Err(e.into()),
    }

    Ok(())
}
