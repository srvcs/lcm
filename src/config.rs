use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub log_level: String,
    pub environment: String,
    /// Base URL of the srvcs-gcd dependency.
    pub gcd_url: String,
    /// Base URL of the srvcs-divide dependency.
    pub divide_url: String,
    /// Base URL of the srvcs-multiply dependency.
    pub multiply_url: String,
}

impl Config {
    #[allow(clippy::too_many_arguments)]
    pub fn from_vars(
        bind: Option<String>,
        log: Option<String>,
        env: Option<String>,
        gcd_url: Option<String>,
        divide_url: Option<String>,
        multiply_url: Option<String>,
    ) -> Self {
        let bind_addr = bind
            .unwrap_or_else(|| "0.0.0.0:8080".to_string())
            .parse()
            .expect("SRVCS_BIND_ADDR must be host:port");
        Config {
            bind_addr,
            log_level: log.unwrap_or_else(|| "info,tower_http=info".to_string()),
            environment: env.unwrap_or_else(|| "development".to_string()),
            gcd_url: gcd_url.unwrap_or_else(|| "http://127.0.0.1:8090".to_string()),
            divide_url: divide_url.unwrap_or_else(|| "http://127.0.0.1:8091".to_string()),
            multiply_url: multiply_url.unwrap_or_else(|| "http://127.0.0.1:8092".to_string()),
        }
    }

    pub fn from_env() -> Self {
        Self::from_vars(
            std::env::var("SRVCS_BIND_ADDR").ok(),
            std::env::var("RUST_LOG").ok(),
            std::env::var("SRVCS_ENV").ok(),
            std::env::var("SRVCS_GCD_URL").ok(),
            std::env::var("SRVCS_DIVIDE_URL").ok(),
            std::env::var("SRVCS_MULTIPLY_URL").ok(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = Config::from_vars(None, None, None, None, None, None);
        assert_eq!(c.bind_addr.port(), 8080);
        assert_eq!(c.gcd_url, "http://127.0.0.1:8090");
        assert_eq!(c.divide_url, "http://127.0.0.1:8091");
        assert_eq!(c.multiply_url, "http://127.0.0.1:8092");
    }

    #[test]
    fn parses_explicit_bind_addr() {
        let c = Config::from_vars(Some("127.0.0.1:9000".into()), None, None, None, None, None);
        assert_eq!(c.bind_addr.port(), 9000);
    }
}
