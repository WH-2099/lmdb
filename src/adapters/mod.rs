pub mod gated;
pub(crate) mod identity;
pub mod modelsdev;
pub mod public;
pub mod vercel;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::model::{Record, Snapshot};

/// Replays a supported source snapshot without fetching or reading credentials.
/// Unknown sources return `None`; their raw pointers can still be validated.
pub fn normalize(value: &Value, snapshot: &Snapshot) -> Result<Option<Vec<Record>>> {
    let records = match snapshot.source.as_str() {
        "huggingface" => public::huggingface(value, snapshot),
        "modelscope" => public::modelscope(value, snapshot),
        "openrouter" => public::openrouter(value, snapshot),
        "vercel" => vercel::models(value, snapshot),
        "modelsdev" => match reqwest::Url::parse(&snapshot.url)?.path() {
            "/models.json" => modelsdev::models(value, snapshot),
            "/catalog.json" => modelsdev::services(value, snapshot),
            path => bail!("unsupported models.dev snapshot path: {path}"),
        },
        source if gated::request_spec(source).is_some() => {
            gated::normalize(source, value, snapshot)
        }
        _ => return Ok(None),
    }
    .with_context(|| format!("snapshot {}: normalize source records", snapshot.id))?;
    Ok(Some(records))
}
