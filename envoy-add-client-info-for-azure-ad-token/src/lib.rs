use proxy_wasm::traits::*;
use proxy_wasm::types::*;
use serde::Deserialize;
use serde_json::Value;

// ──────────────────────────────────────────────
// Configuration
// ──────────────────────────────────────────────

#[derive(Clone, Deserialize, Default)]
struct Config {
    #[serde(default)]
    log_level: String,
}

// ──────────────────────────────────────────────
// Root Context
// ──────────────────────────────────────────────

struct RootCtx {
    config: Config,
}

impl Context for RootCtx {}

impl RootContext for RootCtx {
    fn on_configure(&mut self, _config_size: usize) -> bool {
        log::warn!("AzureAD WASM: RootContext on_configure called!");
        if let Some(bytes) = self.get_plugin_configuration() {
            if let Ok(text) = String::from_utf8(bytes) {
                log::warn!("AzureAD WASM: Config received: {}", text);
                if let Ok(cfg) = serde_json::from_str::<Config>(&text) {
                    self.config = cfg;
                }
            }
        }
        true
    }

    fn create_http_context(&self, _context_id: u32) -> Option<Box<dyn HttpContext>> {
        Some(Box::new(HttpCtx {
            config: self.config.clone(),
        }))
    }

    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }
}

// ──────────────────────────────────────────────
// HTTP Context
// ──────────────────────────────────────────────

struct HttpCtx {
    #[allow(dead_code)]
    config: Config,
}

impl Context for HttpCtx {}

impl HttpContext for HttpCtx {
    fn on_http_request_headers(&mut self, _num_headers: usize, _end_of_stream: bool) -> Action {
        log::warn!("AzureAD WASM: on_http_request_headers called");
        
        // Get Authorization header
        let auth_header = match self.get_http_request_header("authorization") {
            Some(h) => {
                log::warn!("AzureAD WASM: Found Authorization header");
                h
            },
            None => {
                log::warn!("AzureAD WASM: No Authorization header found");
                return Action::Continue;
            }
        };
        
        // Extract Bearer token
        let token = if auth_header.to_lowercase().starts_with("bearer ") {
            auth_header[7..].trim()
        } else {
            return Action::Continue;
        };
        
        // Decode JWT payload (second part)
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            log::warn!("Invalid JWT format");
            return Action::Continue;
        }
        
        let payload = match decode_base64_url(parts[1]) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Failed to decode JWT payload: {}", e);
                return Action::Continue;
            }
        };
        
        let claims: Value = match serde_json::from_str(&payload) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Failed to parse JWT claims: {}", e);
                return Action::Continue;
            }
        };
        
        // Extract and set claims as headers
        log::warn!("AzureAD WASM: JWT decoded successfully, extracting claims");
        
        // x-bayer-user (from given_name + family_name or name)
        let user_name = if let (Some(given), Some(family)) = (
            claims.get("given_name").and_then(|v| v.as_str()),
            claims.get("family_name").and_then(|v| v.as_str()),
        ) {
            format!("{} {}", given, family)
        } else {
            claims.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        
        if !user_name.is_empty() {
            log::warn!("AzureAD WASM: Setting x-bayer-user={}", user_name);
            self.set_http_request_header("x-bayer-user", Some(&user_name));
        }
        
        // x-bayer-cwid (from cwid claim)
        if let Some(cwid) = claims.get("cwid").and_then(|v| v.as_str()) {
            log::warn!("AzureAD WASM: Setting x-bayer-cwid={}", cwid);
            self.set_http_request_header("x-bayer-cwid", Some(cwid));
        }
        
        // oauth_clientid (from appid claim)
        if let Some(appid) = claims.get("appid").and_then(|v| v.as_str()) {
            log::warn!("AzureAD WASM: Setting oauth_clientid={}", appid);
            self.set_http_request_header("oauth_clientid", Some(appid));
        }
        
        // x-bayer-groups (from roles claim as JSON array)
        if let Some(roles) = claims.get("roles") {
            if roles.is_array() {
                let roles_str = roles.to_string();
                log::warn!("AzureAD WASM: Setting x-bayer-groups");
                self.set_http_request_header("x-bayer-groups", Some(&roles_str));
            }
        }
        
        // x-bayer-user-profile (from unique_name)
        if let Some(unique_name) = claims.get("unique_name").and_then(|v| v.as_str()) {
            log::warn!("AzureAD WASM: Setting x-bayer-user-profile={}", unique_name);
            self.set_http_request_header("x-bayer-user-profile", Some(unique_name));
        }
        
        log::info!("Extracted Azure AD claims and set headers");
        
        Action::Continue
    }
}

// ──────────────────────────────────────────────
// Helper Functions
// ──────────────────────────────────────────────

fn decode_base64_url(input: &str) -> Result<String, String> {
    // Add padding if needed
    let padding = 4 - (input.len() % 4);
    let padded = if padding != 4 {
        format!("{}{}", input, "=".repeat(padding))
    } else {
        input.to_string()
    };
    
    // Replace URL-safe characters
    let standard = padded
        .replace('-', "+")
        .replace('_', "/");
    
    // Decode
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    
    let decoded = engine.decode(&standard)
        .map_err(|e| format!("Base64 decode error: {}", e))?;
    
    String::from_utf8(decoded)
        .map_err(|e| format!("UTF-8 decode error: {}", e))
}

// ──────────────────────────────────────────────
// Entry Point
// ──────────────────────────────────────────────

proxy_wasm::main! {{
    proxy_wasm::set_log_level(LogLevel::Info);
    proxy_wasm::set_root_context(|_| -> Box<dyn RootContext> {
        Box::new(RootCtx {
            config: Config::default(),
        })
    });
}}
