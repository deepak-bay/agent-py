# Echo WASM Policy for Envoy Gateway

A lightweight WASM filter that echoes request details back to the client as a JSON response. Designed for Envoy Gateway running on EKS.

## Behavior

| `headers-enabled` | Response includes |
|---|---|
| `true` | method, path, query params, timestamp, **all request headers** (+ body if POST/PUT/PATCH) |
| `false` | method, path, query params, timestamp (+ body if POST/PUT/PATCH) |

`headers-enabled` is configurable per-route via the `EnvoyExtensionPolicy` resource.

---

## Project Structure

```
wasm/echo-policy/
├── Cargo.toml                    # Rust project config
├── README.md                     # This file
├── src/
│   └── lib.rs                    # WASM filter logic
└── k8s/
    └── envoy-extension.yaml      # EnvoyExtensionPolicy manifest
```

---

## Prerequisites

| Tool | Version | Install |
|---|---|---|
| Rust | 1.70+ | https://rustup.rs |
| `wasm32-wasip1` target | — | `rustup target add wasm32-wasip1` |
| kubectl | 1.25+ | https://kubernetes.io/docs/tasks/tools/ |
| EKS cluster with Envoy Gateway | — | Already provisioned |

---

## Step 1 — Install the WASM Build Target

```bash
rustup target add wasm32-wasip1
```

Verify:

```bash
rustup target list --installed | grep wasm
# should show: wasm32-wasip1
```

---

## Step 2 — Build the WASM Binary

```bash
cd wasm/echo-policy

cargo build --target wasm32-wasip1 --release
```

The compiled binary will be at:

```
target/wasm32-wasip1/release/echo_policy.wasm
```

Check the file size (should be a few hundred KB):

```bash
ls -lh target/wasm32-wasip1/release/echo_policy.wasm
```

---

## Step 3 — Debug / Verify Locally (Optional)

### 3a. Check for compile errors

```bash
cargo check --target wasm32-wasip1
```

### 3b. View Envoy proxy logs at runtime

Once deployed (steps below), tail the Envoy Gateway proxy logs to see any `proxy_wasm` log output:

```bash
# Find the envoy proxy pod(s)
kubectl get pods -n envoy-gateway-system

# Tail logs
kubectl logs -f <envoy-proxy-pod> -n envoy-gateway-system
```

Look for lines containing `echo-policy` — any `proxy_wasm::set_log_level(LogLevel::Info)` messages will appear here.

### 3c. Inspect the WASM binary

```bash
# Show exported functions (requires wasmtime or wasm-tools)
wasm-tools print target/wasm32-wasip1/release/echo_policy.wasm | head -30
```

---

## Step 4 — Deploy to EKS

### 4a. Upload the WASM binary to S3

```bash
aws s3 cp target/wasm32-wasip1/release/echo_policy.wasm \
  s3://your-bucket/wasm/echo_policy.wasm
```

The Envoy Gateway `EnvoyExtensionPolicy` CRD only supports `HTTP` and `Image` for `code.type` (**not** `ConfigMap`). Using S3 with an HTTP URL is the simplest approach.

> Make sure the S3 object is accessible from the EKS cluster (public-read, pre-signed URL, or VPC endpoint).

To **update** after a rebuild, simply upload again:

```bash
aws s3 cp target/wasm32-wasip1/release/echo_policy.wasm \
  s3://your-bucket/wasm/echo_policy.wasm
```

Optionally compute a sha256 for integrity verification:

```bash
shasum -a 256 target/wasm32-wasip1/release/echo_policy.wasm
```

### 4b. Edit the EnvoyExtensionPolicy

Open `k8s/envoy-extension.yaml` and update:

| Field | What to change |
|---|---|
| `metadata.namespace` | Your namespace |
| `spec.targetRefs[0].name` | Name of the `HTTPRoute` to attach to |
| `spec.wasm[0].code.http.url` | Your S3 URL (e.g. `https://your-bucket.s3.amazonaws.com/wasm/echo_policy.wasm`) |
| `spec.wasm[0].config.headers-enabled` | `true` or `false` |

### 4c. Apply the policy

```bash
kubectl apply -f k8s/envoy-extension.yaml
```

Verify it was accepted:

```bash
kubectl get envoyextensionpolicy echo-policy -n default -o yaml
```

---

## Step 5 — Test

### GET request (no body)

```bash
curl -v "http://<GATEWAY_URL>/your-path?foo=bar&baz=123"
```

**Expected response (`headers-enabled: true`):**

```json
{
  "method": "GET",
  "path": "/your-path",
  "queryParams": {
    "foo": "bar",
    "baz": "123"
  },
  "timestamp": 1772000000,
  "headers": {
    "host": "example.com",
    "user-agent": "curl/8.x",
    "accept": "*/*"
  }
}
```

**Expected response (`headers-enabled: false`):**

```json
{
  "method": "GET",
  "path": "/your-path",
  "queryParams": {
    "foo": "bar",
    "baz": "123"
  },
  "timestamp": 1772000000
}
```

### POST request (with body)

```bash
curl -X POST "http://<GATEWAY_URL>/your-path" \
  -H "Content-Type: application/json" \
  -d '{"key": "value"}'
```

**Expected response (`headers-enabled: true`):**

```json
{
  "method": "POST",
  "path": "/your-path",
  "queryParams": {},
  "timestamp": 1772000000,
  "headers": {
    "host": "example.com",
    "content-type": "application/json",
    "user-agent": "curl/8.x"
  },
  "body": {
    "key": "value"
  }
}
```

---

## Attaching to Multiple Routes

Create one `EnvoyExtensionPolicy` per route, each with its own `headers-enabled` value:

```yaml
# Route A — echo with headers
apiVersion: gateway.envoyproxy.io/v1alpha1
kind: EnvoyExtensionPolicy
metadata:
  name: echo-policy-route-a
spec:
  targetRefs:
    - group: gateway.networking.k8s.io
      kind: HTTPRoute
      name: route-a
  wasm:
    - name: echo-policy
      code:
        type: HTTP
        http:
          url: https://your-bucket.s3.amazonaws.com/wasm/echo_policy.wasm
      config:
        headers-enabled: true
---
# Route B — echo without headers
apiVersion: gateway.envoyproxy.io/v1alpha1
kind: EnvoyExtensionPolicy
metadata:
  name: echo-policy-route-b
spec:
  targetRefs:
    - group: gateway.networking.k8s.io
      kind: HTTPRoute
      name: route-b
  wasm:
    - name: echo-policy
      code:
        type: HTTP
        http:
          url: https://your-bucket.s3.amazonaws.com/wasm/echo_policy.wasm
      config:
        headers-enabled: false
```

---

## Cleanup

```bash
# Remove the policy
kubectl delete envoyextensionpolicy echo-policy -n default

# Optionally remove WASM from S3
aws s3 rm s3://your-bucket/wasm/echo_policy.wasm
```

---

## Quick Reference

| Action | Command |
|---|---|
| Build | `cargo build --target wasm32-wasip1 --release` |
| Check errors | `cargo check --target wasm32-wasip1` |
| Upload to S3 | `aws s3 cp target/wasm32-wasip1/release/echo_policy.wasm s3://your-bucket/wasm/echo_policy.wasm` |
| Apply policy | `kubectl apply -f k8s/envoy-extension.yaml` |
| View proxy logs | `kubectl logs -f <envoy-pod> -n envoy-gateway-system` |
| Remove policy | `kubectl delete envoyextensionpolicy echo-policy` |
