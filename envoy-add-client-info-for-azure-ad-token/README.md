# envoy-add-client-info-for-azure-ad-token

Envoy Gateway WASM policy that decodes Azure AD JWT tokens and extracts claims as custom headers.

## Functionality

1. Extracts Bearer token from `Authorization` header
2. Base64 decodes JWT payload (no signature verification)
3. Extracts claims and sets them as headers:
   - `x-bayer-user`: from `given_name` + `family_name` or `name`
   - `x-bayer-cwid`: from `cwid` claim
   - `oauth_clientid`: from `appid` claim
   - `x-bayer-groups`: from `roles` claim (as JSON array)
   - `x-bayer-user-profile`: from `unique_name` claim

## Configuration

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `log_level` | string | No | Logging level (default: info) |

## Example Configuration

```yaml
apiVersion: gateway.envoyproxy.io/v1alpha1
kind: EnvoyExtensionPolicy
metadata:
  name: add-client-info-for-azure-ad-token
spec:
  targetRefs:
    - group: gateway.networking.k8s.io
      kind: HTTPRoute
      name: my-route
  wasm:
    - name: add-client-info-for-azure-ad-token
      code:
        type: HTTP
        http:
          url: https://your-bucket.s3.amazonaws.com/envoy_add_client_info_for_azure_ad_token.wasm
      config:
        log_level: "info"
```

## Security Note

This policy decodes JWT tokens without validating the signature. It should be used **after** proper JWT validation has been performed (e.g., by Envoy's JWT authentication filter).

## Build

```bash
cargo build --target wasm32-wasip1 --release
```
