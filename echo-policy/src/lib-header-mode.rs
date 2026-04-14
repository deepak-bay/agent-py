// ═══════════════════════════════════════════════════════════
// Echo Policy — Header Mode (Composable Version)
//
// Instead of sending an immediate response, this version adds
// request metadata as headers and allows the filter chain to
// continue to upstream.
// ═══════════════════════════════════════════════════════════

use proxy_wasm::traits::*;
use proxy_wasm::types::*;
use serde_json::Value;

#[derive(Clone)]
struct EchoConfig {
    enabled: bool,
}

impl Default for EchoConfig {
    fn default() -> Self {
        EchoConfig { enabled: true }
    }
}

struct EchoRoot {
    config: EchoConfig,
}

impl Context for EchoRoot {}

impl RootContext for EchoRoot {
    fn on_configure(&mut self, _config_size: usize) -> bool {
        if let Some(bytes) = self.get_plugin_configuration() {
            if let Ok(text) = String::from_utf8(bytes) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if let Some(enabled) = v.get("headers-enabled") {
                        self.config.enabled = enabled.as_bool().unwrap_or(true);
                    }
                }
            }
        }
        true
    }

    fn create_http_context(&self, _context_id: u32) -> Option<Box<dyn HttpContext>> {
        Some(Box::new(EchoHttp {
            config: self.config.clone(),
        }))
    }

    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }
}

struct EchoHttp {
    config: EchoConfig,
}

impl Context for EchoHttp {}

impl HttpContext for EchoHttp {
    fn on_http_request_headers(&mut self, _num_headers: usize, _end_of_stream: bool) -> Action {
        if !self.config.enabled {
            return Action::Continue;
        }

        // Add echo metadata as debug headers
        if let Some(method) = self.get_http_request_header(":method") {
            self.set_http_request_header("x-echo-method", Some(&method));
        }
        
        if let Some(path) = self.get_http_request_header(":path") {
            self.set_http_request_header("x-echo-path", Some(&path));
        }
        
        if let Some(authority) = self.get_http_request_header(":authority") {
            self.set_http_request_header("x-echo-authority", Some(&authority));
        }

        // Add timestamp
        let ts = self
            .get_current_time()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_secs();
        self.set_http_request_header("x-echo-timestamp", Some(&ts.to_string()));

        log::info!("Echo policy: added debug headers");
        
        // ✅ CRITICAL: Return Continue to allow next filter to run
        Action::Continue
    }
}

proxy_wasm::main! {{
    proxy_wasm::set_log_level(LogLevel::Info);
    proxy_wasm::set_root_context(|_| -> Box<dyn RootContext> {
        Box::new(EchoRoot {
            config: EchoConfig::default(),
        })
    });
}}
