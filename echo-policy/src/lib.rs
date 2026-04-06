use proxy_wasm::traits::*;
use proxy_wasm::types::*;
use serde_json::{json, Map, Value};
use std::time::{Duration, UNIX_EPOCH};

// ──────────────────────────────────────────────
// 1.  Root Context – parses per-route config
// ──────────────────────────────────────────────

/// Plugin configuration passed via EnvoyExtensionPolicy `configuration` field.
/// Example: {"headers-enabled": true}
#[derive(Clone)]
struct EchoConfig {
    headers_enabled: bool,
}

impl Default for EchoConfig {
    fn default() -> Self {
        EchoConfig {
            headers_enabled: true, // safe default: echo everything
        }
    }
}

struct EchoRoot {
    config: EchoConfig,
}

impl Context for EchoRoot {}

impl RootContext for EchoRoot {
    // Called once when the plugin configuration (JSON string) is loaded.
    fn on_configure(&mut self, _config_size: usize) -> bool {
        if let Some(bytes) = self.get_plugin_configuration() {
            if let Ok(text) = String::from_utf8(bytes) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if let Some(enabled) = v.get("headers-enabled") {
                        self.config.headers_enabled =
                            enabled.as_bool().unwrap_or(true);
                    }
                }
            }
        }
        true
    }

    fn create_http_context(&self, _context_id: u32) -> Option<Box<dyn HttpContext>> {
        Some(Box::new(EchoHttp {
            config: self.config.clone(),
            request_body: Vec::new(),
        }))
    }

    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }
}

// ──────────────────────────────────────────────
// 2.  HTTP Context – runs per request
// ──────────────────────────────────────────────

struct EchoHttp {
    config: EchoConfig,
    request_body: Vec<u8>,
}

impl Context for EchoHttp {}

impl EchoHttp {
    /// Build the JSON echo response.
    fn build_echo_response(&self, headers: Vec<(String, String)>) -> String {
        let mut echo = Map::new();

        // ── Always included fields ──────────────────────────

        // :method
        let method = headers
            .iter()
            .find(|(k, _)| k == ":method")
            .map(|(_, v)| v.as_str())
            .unwrap_or("GET");
        echo.insert("method".into(), json!(method));

        // :path  (may contain query string)
        let full_path = headers
            .iter()
            .find(|(k, _)| k == ":path")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();

        // Split path and query
        let (path, query_string) = match full_path.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (full_path.clone(), String::new()),
        };
        echo.insert("path".into(), json!(path));

        // Query params as object
        let mut qp = Map::new();
        if !query_string.is_empty() {
            for pair in query_string.split('&') {
                let mut kv = pair.splitn(2, '=');
                let key = kv.next().unwrap_or_default();
                let val = kv.next().unwrap_or_default();
                qp.insert(key.to_string(), json!(val));
            }
        }
        echo.insert("queryParams".into(), Value::Object(qp));

        // Timestamp (epoch seconds from Envoy clock)
        let ts = self
            .get_current_time()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        echo.insert("timestamp".into(), json!(ts));

        // ── Conditional: request headers ────────────────────
        if self.config.headers_enabled {
            let mut hdr_map = Map::new();
            for (k, v) in &headers {
                // skip Envoy pseudo-headers already captured above
                if k.starts_with(':') {
                    continue;
                }
                hdr_map.insert(k.clone(), json!(v));
            }
            echo.insert("headers".into(), Value::Object(hdr_map));
        }

        // ── Body (for POST / PUT / PATCH) ───────────────────
        let is_body_method = matches!(method, "POST" | "PUT" | "PATCH");
        if is_body_method && !self.request_body.is_empty() {
            // Try to parse as JSON, fall back to plain string
            let body_str = String::from_utf8_lossy(&self.request_body);
            if let Ok(body_json) = serde_json::from_str::<Value>(&body_str) {
                echo.insert("body".into(), body_json);
            } else {
                echo.insert("body".into(), json!(body_str));
            }
        }

        serde_json::to_string_pretty(&Value::Object(echo)).unwrap_or_default()
    }
}

impl HttpContext for EchoHttp {
    fn on_http_request_headers(&mut self, _num_headers: usize, _end_of_stream: bool) -> Action {
        let method = self
            .get_http_request_header(":method")
            .unwrap_or_default();

        // For body-carrying methods, wait for the body before responding.
        if matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
            return Action::Continue; // body will arrive in on_http_request_body
        }

        // Non-body methods: respond immediately
        let headers = self.get_http_request_headers();
        let body = self.build_echo_response(headers);
        self.send_http_response(200, vec![("content-type", "application/json")], Some(body.as_bytes()));
        Action::Pause
    }

    fn on_http_request_body(&mut self, body_size: usize, end_of_stream: bool) -> Action {
        // Accumulate body bytes
        if let Some(chunk) = self.get_http_request_body(0, body_size) {
            self.request_body.extend_from_slice(&chunk);
        }

        if !end_of_stream {
            return Action::Pause; // wait for rest of body
        }

        // Full body received — respond
        let headers = self.get_http_request_headers();
        let body = self.build_echo_response(headers);
        self.send_http_response(200, vec![("content-type", "application/json")], Some(body.as_bytes()));
        Action::Pause
    }
}

// ──────────────────────────────────────────────
// 3.  Entry point
// ──────────────────────────────────────────────

proxy_wasm::main! {{
    proxy_wasm::set_log_level(LogLevel::Info);
    proxy_wasm::set_root_context(|_| -> Box<dyn RootContext> {
        Box::new(EchoRoot {
            config: EchoConfig::default(),
        })
    });
}}
