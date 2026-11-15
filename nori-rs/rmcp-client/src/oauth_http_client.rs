//! Adapter for oauth2's HTTP interface using the MCP transport's reqwest client.

pub(crate) type McpOAuthClient = oauth2::Client<
    oauth2::basic::BasicErrorResponse,
    rmcp::transport::auth::OAuthTokenResponse,
    oauth2::basic::BasicTokenIntrospectionResponse,
    oauth2::StandardRevocableToken,
    oauth2::basic::BasicRevocationErrorResponse,
>;

pub(crate) async fn request(
    client: reqwest::Client,
    request: oauth2::HttpRequest,
) -> Result<oauth2::HttpResponse, oauth2::HttpClientError<reqwest::Error>> {
    let request = reqwest::Request::try_from(request).map_err(Box::new)?;
    let mut response = client.execute(request).await.map_err(Box::new)?;
    let mut builder = oauth2::http::Response::builder()
        .status(response.status())
        .version(response.version());
    for (name, value) in response.headers() {
        builder = builder.header(name, value);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(Box::new)? {
        if chunk.len() > 1024 * 1024 - body.len() {
            return Err(oauth2::HttpClientError::Other(
                "OAuth response exceeds 1 MiB".into(),
            ));
        }
        body.extend_from_slice(&chunk);
    }
    builder.body(body).map_err(oauth2::HttpClientError::Http)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::post;
    use oauth2::TokenResponse;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn exchanges_code_and_preserves_vendor_fields_and_oauth_errors() -> anyhow::Result<()> {
        for (status, body) in [
            (
                StatusCode::OK,
                json!({"access_token": "local-token", "token_type": "Bearer", "vendor": "extra"}),
            ),
            (StatusCode::BAD_REQUEST, json!({"error": "invalid_grant"})),
        ] {
            let router = Router::new().route(
                "/token",
                post(
                    move |headers: axum::http::HeaderMap, request_body: String| async move {
                        assert_eq!(headers["x-nori-test"], "custom");
                        assert!(request_body.contains("code=local-code"));
                        assert!(request_body.contains("code_verifier=local-verifier"));
                        (
                            status,
                            [("content-type", "application/json")],
                            body.to_string(),
                        )
                    },
                ),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let url = format!("http://{}/token", listener.local_addr()?);
            let server = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            let client = McpOAuthClient::new(oauth2::ClientId::new("local-client".into()))
                .set_token_uri(oauth2::TokenUrl::new(url)?);
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .default_headers(
                    [(
                        reqwest::header::HeaderName::from_static("x-nori-test"),
                        reqwest::header::HeaderValue::from_static("custom"),
                    )]
                    .into_iter()
                    .collect(),
                )
                .build()?;
            let adapter = |req| request(http.clone(), req);
            let result = client
                .exchange_code(oauth2::AuthorizationCode::new("local-code".into()))
                .set_pkce_verifier(oauth2::PkceCodeVerifier::new("local-verifier".into()))
                .request_async(&adapter)
                .await;
            server.abort();
            if status.is_success() {
                let token = result?;
                assert_eq!(token.access_token().secret(), "local-token");
                assert_eq!(token.extra_fields().0.get("vendor"), Some(&json!("extra")));
            } else {
                let oauth2::RequestTokenError::ServerResponse(error) = result.unwrap_err() else {
                    panic!("expected OAuth error response");
                };
                assert_eq!(
                    error.error(),
                    &oauth2::basic::BasicErrorResponseType::InvalidGrant
                );
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn rejects_oversized_chunked_token_response() -> anyhow::Result<()> {
        let router = Router::new().route(
            "/token",
            post(|| async {
                axum::body::Body::from_stream(futures::stream::iter(
                    (0..=1024).map(|_| Ok::<_, std::io::Error>(" ".repeat(1024))),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/token", listener.local_addr()?);
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let req = oauth2::http::Request::post(url).body(Vec::new())?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        let result = request(http, req).await;
        server.abort();
        assert!(
            matches!(result, Err(oauth2::HttpClientError::Other(message)) if message == "OAuth response exceeds 1 MiB")
        );
        Ok(())
    }
}
