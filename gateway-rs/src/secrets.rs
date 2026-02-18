use std::time::Duration;
use tokio::process::Command;

/// Load a secret from `pass` with a 2-second timeout.
///
/// Returns `None` if:
/// - `pass` is not installed
/// - GPG is locked (the command times out)
/// - The secret path doesn't exist
/// - Any other error occurs
///
/// This matches the Go `loadFromPass()` behavior: fail silently, never hang.
pub async fn load_from_pass(path: &str) -> Option<String> {
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        Command::new("pass").arg(path).output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let secret = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if secret.is_empty() {
                None
            } else {
                Some(secret)
            }
        }
        Ok(Ok(_)) => {
            // pass exited with non-zero (GPG locked, path not found, etc.)
            None
        }
        Ok(Err(e)) => {
            tracing::debug!(error = %e, path, "pass command failed to execute");
            None
        }
        Err(_) => {
            tracing::debug!(path, "pass command timed out (GPG likely locked)");
            None
        }
    }
}
