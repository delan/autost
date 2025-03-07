use std::time::Duration;

use bytes::Bytes;
use jane_eyre::eyre::{self, bail};
use reqwest::{Client, Response};
use serde::de::DeserializeOwned;
use tokio::time::sleep;
use tracing::{info, warn};

pub async fn get_json<T: DeserializeOwned>(client: &Client, url: &str) -> eyre::Result<T> {
    get_with_retries(client, url, |body| json(&body)).await
}

pub async fn get_with_retries<T>(
    client: &Client,
    url: &str,
    mut and_then: impl FnMut(Bytes) -> eyre::Result<T>,
) -> eyre::Result<T> {
    let mut retries = 4;
    let mut wait = Duration::from_secs(4);
    loop {
        let result = get_response_once(client, url).await;
        let status = result
            .as_ref()
            .map_or(None, |response| Some(response.status()));
        let result = match match result {
            Ok(response) => Ok(response.bytes().await),
            Err(error) => Err(error),
        } {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(error)) | Err(error) => Err::<Bytes, eyre::Report>(error.into()),
        };
        // retry requests if they are neither client errors (http 4xx), nor if they are successful
        // (http 2xx) and the given fallible transformation fails. this includes server errors
        // (http 5xx), and requests that failed in a way that yields no response.
        let error = if status.is_some_and(|s| s.is_client_error()) {
            // client errors (http 4xx) should not be retried.
            bail!("GET request failed (no retries): http {:?}: {url}", status);
        } else if status.is_some_and(|s| s.is_success()) {
            // apply the given fallible transformation to the response body.
            // if that succeeds, we succeed, otherwise we retry.
            let result = result.and_then(&mut and_then);
            if result.is_ok() {
                return result;
            }
            result.err()
        } else {
            // when retrying server errors (http 5xx), error is None.
            // when retrying failures with no response, error is Some.
            result.err()
        };
        if retries == 0 {
            bail!(
                "GET request failed (after retries): http {:?}: {url}",
                status,
            );
        }
        warn!(?wait, ?status, url, ?error, "retrying failed GET request");
        sleep(wait).await;
        wait *= 2;
        retries -= 1;
    }
}

async fn get_response_once(client: &Client, url: &str) -> reqwest::Result<Response> {
    info!("GET {url}");
    client.get(url).send().await
}

fn json<T: DeserializeOwned>(body: &Bytes) -> eyre::Result<T> {
    Ok(serde_json::from_slice(body)?)
}
