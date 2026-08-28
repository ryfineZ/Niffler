use std::{
    net::{IpAddr, SocketAddr},
    sync::LazyLock,
};

use axum::{
    extract::{ConnectInfo, Request},
    middleware::Next,
    response::Response,
    Router,
};
use http::{header::HeaderName, HeaderMap};

/// Cloudflare-specific headers that are not part of the `cf-*` prefix family.
const CF_EXACT_HEADERS: &[&str] = &["cdn-loop", "true-client-ip"];
const TRUSTED_PROXY_CIDRS_ENV: &str = "AETHER_TRUSTED_PROXY_CIDRS";
const DEFAULT_TRUSTED_PROXY_CIDRS: &str = "127.0.0.0/8,::1/128";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrustedClientIp(pub(crate) String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IpCidr {
    V4 { network: u32, prefix: u8 },
    V6 { network: u128, prefix: u8 },
}

impl IpCidr {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        let (address, prefix) = match value.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (value, None),
        };
        match address.trim().parse::<IpAddr>().ok()? {
            IpAddr::V4(address) => {
                let prefix = prefix
                    .map(str::trim)
                    .map(str::parse::<u8>)
                    .transpose()
                    .ok()?
                    .unwrap_or(32);
                if prefix > 32 {
                    return None;
                }
                let mask = ipv4_mask(prefix);
                Some(Self::V4 {
                    network: u32::from(address) & mask,
                    prefix,
                })
            }
            IpAddr::V6(address) => {
                let prefix = prefix
                    .map(str::trim)
                    .map(str::parse::<u8>)
                    .transpose()
                    .ok()?
                    .unwrap_or(128);
                if prefix > 128 {
                    return None;
                }
                let mask = ipv6_mask(prefix);
                Some(Self::V6 {
                    network: u128::from(address) & mask,
                    prefix,
                })
            }
        }
    }

    fn contains(self, address: IpAddr) -> bool {
        match (self, address) {
            (Self::V4 { network, prefix }, IpAddr::V4(address)) => {
                u32::from(address) & ipv4_mask(prefix) == network
            }
            (Self::V6 { network, prefix }, IpAddr::V6(address)) => {
                u128::from(address) & ipv6_mask(prefix) == network
            }
            _ => false,
        }
    }
}

fn ipv4_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

fn ipv6_mask(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    }
}

fn parse_trusted_proxy_cidrs(value: &str) -> Vec<IpCidr> {
    value
        .split(',')
        .filter_map(|raw| {
            let parsed = IpCidr::parse(raw);
            if parsed.is_none() && !raw.trim().is_empty() {
                tracing::warn!(
                    value = raw.trim(),
                    env = TRUSTED_PROXY_CIDRS_ENV,
                    "ignoring invalid trusted proxy CIDR"
                );
            }
            parsed
        })
        .collect()
}

static TRUSTED_PROXY_CIDRS: LazyLock<Vec<IpCidr>> = LazyLock::new(|| {
    let configured = std::env::var(TRUSTED_PROXY_CIDRS_ENV)
        .unwrap_or_else(|_| DEFAULT_TRUSTED_PROXY_CIDRS.to_string());
    parse_trusted_proxy_cidrs(&configured)
});

fn should_strip_cf_header(name: &HeaderName) -> bool {
    let normalized = name.as_str();
    normalized.starts_with("cf-") || CF_EXACT_HEADERS.contains(&normalized)
}

fn header_ip(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("unknown"))
        .and_then(|value| value.parse::<IpAddr>().ok())
}

fn forwarded_client_ip(headers: &HeaderMap, trusted_proxies: &[IpCidr]) -> Option<IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .into_iter()
        .flat_map(|value| value.split(',').rev())
        .filter_map(|value| value.trim().parse::<IpAddr>().ok())
        .find(|address| {
            !trusted_proxies
                .iter()
                .any(|trusted| trusted.contains(*address))
        })
        .or_else(|| header_ip(headers, "x-real-ip"))
        // CF-Connecting-IP is only a final fallback. A trusted reverse proxy's
        // sanitized X-Forwarded-For prevents a direct client from overriding it.
        .or_else(|| header_ip(headers, "cf-connecting-ip"))
}

fn trusted_client_ip(headers: &HeaderMap, peer_ip: IpAddr, trusted_proxies: &[IpCidr]) -> IpAddr {
    if trusted_proxies
        .iter()
        .any(|trusted| trusted.contains(peer_ip))
    {
        forwarded_client_ip(headers, trusted_proxies).unwrap_or(peer_ip)
    } else {
        peer_ip
    }
}

fn strip_cf_headers(headers: &mut HeaderMap) {
    let to_remove: Vec<_> = headers
        .keys()
        .filter(|name| should_strip_cf_header(name))
        .cloned()
        .collect();
    for name in to_remove {
        headers.remove(name);
    }
}

pub(crate) fn apply_cf_header_stripping(router: Router) -> Router {
    router.layer(axum::middleware::from_fn(strip_cf_headers_middleware))
}

pub async fn strip_cf_headers_middleware(mut request: Request, next: Next) -> Response {
    if request.extensions().get::<TrustedClientIp>().is_none() {
        if let Some(peer_addr) = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|value| value.0)
        {
            let client_ip =
                trusted_client_ip(request.headers(), peer_addr.ip(), &TRUSTED_PROXY_CIDRS);
            request
                .extensions_mut()
                .insert(TrustedClientIp(client_ip.to_string()));
        }
    }
    strip_cf_headers(request.headers_mut());

    let mut response = next.run(request).await;

    strip_cf_headers(response.headers_mut());

    response
}

#[cfg(test)]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::extract::{ConnectInfo, Extension};
    use axum::routing::any;
    use axum::Router;
    use http::{HeaderValue, Request, Response};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tower::ServiceExt;

    use super::{
        apply_cf_header_stripping, parse_trusted_proxy_cidrs, trusted_client_ip, TrustedClientIp,
    };

    fn request_from_peer(peer: [u8; 4]) -> Request<Body> {
        let mut request = Request::builder()
            .uri("/")
            .header("cf-connecting-ip", "203.0.113.10")
            .header("x-forwarded-for", "198.51.100.20")
            .body(Body::empty())
            .expect("request should build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((peer, 40000))));
        request
    }

    #[tokio::test]
    async fn strips_cf_prefixed_and_exact_headers_from_request_and_response() {
        let app =
            apply_cf_header_stripping(
                Router::new().route(
                    "/",
                    any(
                        |headers: http::HeaderMap,
                         Extension(client_ip): Extension<TrustedClientIp>| async move {
                            let leaked = headers.contains_key("cf-ipcity")
                                || headers.contains_key("cf-ray")
                                || headers.contains_key("cf-connecting-ip")
                                || headers.contains_key("true-client-ip")
                                || headers.contains_key("cdn-loop");
                            let body = if leaked {
                                "leaked".to_string()
                            } else {
                                format!("clean:{}", client_ip.0)
                            };
                            let mut response = Response::new(Body::from(body));
                            response.headers_mut().insert(
                                http::header::HeaderName::from_static("cf-ipcity"),
                                HeaderValue::from_static("Shanghai"),
                            );
                            response.headers_mut().insert(
                                http::header::HeaderName::from_static("cf-cache-status"),
                                HeaderValue::from_static("HIT"),
                            );
                            response.headers_mut().insert(
                                http::header::HeaderName::from_static("true-client-ip"),
                                HeaderValue::from_static("1.1.1.1"),
                            );
                            response.headers_mut().insert(
                                http::header::HeaderName::from_static("cdn-loop"),
                                HeaderValue::from_static("cloudflare"),
                            );
                            response
                        },
                    ),
                ),
            );

        let mut request = request_from_peer([127, 0, 0, 1]);
        request
            .headers_mut()
            .insert("cf-ipcity", HeaderValue::from_static("Shanghai"));
        request
            .headers_mut()
            .insert("cf-ray", HeaderValue::from_static("abc123"));
        request
            .headers_mut()
            .insert("true-client-ip", HeaderValue::from_static("1.1.1.1"));
        request
            .headers_mut()
            .insert("cdn-loop", HeaderValue::from_static("cloudflare"));
        let response = app.oneshot(request).await.expect("request should succeed");

        assert!(response.headers().get("cf-ipcity").is_none());
        assert!(response.headers().get("cf-cache-status").is_none());
        assert!(response.headers().get("cf-connecting-ip").is_none());
        assert!(response.headers().get("true-client-ip").is_none());
        assert!(response.headers().get("cdn-loop").is_none());

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        assert_eq!(body.as_ref(), b"clean:198.51.100.20");
    }

    #[tokio::test]
    async fn preserves_non_cf_headers() {
        let app = apply_cf_header_stripping(Router::new().route(
            "/",
            any(|headers: http::HeaderMap| async move {
                let mut response = Response::new(Body::from(
                    headers
                        .get("x-custom-header")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_string(),
                ));
                response.headers_mut().insert(
                    http::header::HeaderName::from_static("x-custom-response"),
                    HeaderValue::from_static("kept"),
                );
                response
            }),
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("x-custom-header", "kept")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(
            response
                .headers()
                .get("x-custom-response")
                .and_then(|value| value.to_str().ok()),
            Some("kept")
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        assert_eq!(body.as_ref(), b"kept");
    }

    #[test]
    fn untrusted_peer_cannot_spoof_forwarded_client_ip() {
        let headers = request_from_peer([198, 51, 100, 30]).into_parts().0.headers;
        let trusted = parse_trusted_proxy_cidrs("127.0.0.0/8,::1/128");

        assert_eq!(
            trusted_client_ip(
                &headers,
                IpAddr::V4(Ipv4Addr::new(198, 51, 100, 30)),
                &trusted,
            ),
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 30))
        );
    }

    #[test]
    fn trusted_proxy_can_supply_client_ip() {
        let headers = request_from_peer([127, 0, 0, 1]).into_parts().0.headers;
        let trusted = parse_trusted_proxy_cidrs("127.0.0.0/8,::1/128");

        assert_eq!(
            trusted_client_ip(&headers, IpAddr::V4(Ipv4Addr::LOCALHOST), &trusted,),
            "198.51.100.20".parse::<IpAddr>().expect("IP should parse")
        );
    }

    #[test]
    fn trusted_proxy_cidr_parser_supports_ipv4_and_ipv6() {
        let trusted = parse_trusted_proxy_cidrs("10.0.0.0/8,2001:db8::/32");

        assert!(trusted
            .iter()
            .any(|cidr| cidr.contains("10.2.3.4".parse().expect("IP should parse"))));
        assert!(trusted
            .iter()
            .any(|cidr| cidr.contains("2001:db8::1234".parse().expect("IP should parse"))));
        assert!(!trusted
            .iter()
            .any(|cidr| cidr.contains("192.0.2.1".parse().expect("IP should parse"))));
    }
}
