---
name: workos-auth
description: "WorkOS auth.md — how an agent authenticates to a service via agentic registration (discover, register, claim with OTP when there is no user identity or no ID-JAG, exchange an ID-JAG for an access token, call the API, handle revocation). Load before implementing agent-to-service authentication, or when an API returns 401 with a WWW-Authenticate resource_metadata pointer."
---

# auth.md

You are an agent. This service supports **agentic registration**: discover → register → (claim if needed) → exchange for an access_token → call API → handle revocation. Follow the steps in order; do not skip ahead.

Examples use placeholder hosts: `https://api.service.example.com` (the resource server hosting the API you want to call) and `https://auth.service.example.com` (the authorization server that handles registration). Real services substitute their own.

## Step 1 — Discover

Discovery is two hops — you may have already done this.

The 401 response that pointed you here also carries a `WWW-Authenticate` header with the PRM URL:

```http
HTTP/1.1 401 Unauthorized
WWW-Authenticate: Bearer resource_metadata="https://api.service.example.com/.well-known/oauth-protected-resource"
```

Pull the `resource_metadata` value from that header and fetch it (1a). If you don't have the 401 in hand, the conventional path on the resource server is `/.well-known/oauth-protected-resource`.

### 1a. Fetch the Protected Resource Metadata

```http
GET /.well-known/oauth-protected-resource
```

Response shape:

```json
{
  "resource": "https://api.service.example.com/",
  "resource_name": "Service",
  "resource_logo_uri": "https://service.example.com/logo.png",
  "authorization_servers": ["https://auth.service.example.com/"],
  "scopes_supported": ["api.read", "api.write"],
  "bearer_methods_supported": ["header"]
}
```

What each field tells you:

- `resource` — the canonical URL of the API you're trying to call. Use this as the `aud` when minting an ID-JAG.
- `resource_name` / `resource_logo_uri` — display name and logo for the service. Surface these to the user when asking for consent.
- `authorization_servers` — base URLs of the OAuth Authorization Server(s) for this resource. The `agent_auth` block lives on one of these (see 1b).
- `scopes_supported` — scopes the resource server understands. The access_token you receive at Step 5 will be scoped to some subset.
- `bearer_methods_supported` — how you'll send the access_token in Step 6 (`"header"` = `Authorization: Bearer …`).

### 1b. Fetch the Authorization Server metadata

```http
GET <authorization_servers[0]>/.well-known/oauth-authorization-server
```

Response shape:

```json
{
  "resource": "https://api.service.example.com/",
  "authorization_servers": ["https://auth.service.example.com/"],
  "scopes_supported": ["api.read", "api.write"],
  "bearer_methods_supported": ["header"],

  "issuer": "https://auth.service.example.com",
  "token_endpoint": "https://auth.service.example.com/oauth2/token",
  "revocation_endpoint": "https://auth.service.example.com/oauth2/revoke",
  "grant_types_supported": ["urn:ietf:params:oauth:grant-type:jwt-bearer"],

  "agent_auth": {
    "skill": "https://service.example.com/auth.md",
    "identity_endpoint": "https://auth.service.example.com/agent/identity",
    "claim_endpoint": "https://auth.service.example.com/agent/identity/claim",
    "revocation_uri": "https://auth.service.example.com/agent/auth/revoke",
    "identity_types_supported": ["anonymous", "identity_assertion"],
    "identity_assertion": {
      "assertion_types_supported": [
        "urn:ietf:params:oauth:token-type:id-jag",
        "verified_email"
      ]
    },
    "events_supported": [
      "https://schemas.workos.com/events/agent/auth/identity/assertion/revoked"
    ]
  }
}
```

The outer fields restate the PRM. The top-level OAuth endpoints (`issuer`, `token_endpoint`, `revocation_endpoint`, `grant_types_supported`) are standard [RFC 8414](https://datatracker.ietf.org/doc/html/rfc8414) / [RFC 7009](https://datatracker.ietf.org/doc/html/rfc7009) / [RFC 7523](https://datatracker.ietf.org/doc/html/rfc7523) fields. The `agent_auth` block is the profile-specific bootstrap surface — read it in full. Every field is relevant:

- `issuer` — the canonical issuer URL of this authorization server. Validate the `iss` claim of any token the AS signs against this.
- `token_endpoint` — where you exchange a service-signed identity assertion for an access_token (Step 5).
- `revocation_endpoint` — where you POST to revoke an access_token ([RFC 7009](https://datatracker.ietf.org/doc/html/rfc7009)).
- `grant_types_supported` — confirms this AS accepts `urn:ietf:params:oauth:grant-type:jwt-bearer` ([RFC 7523](https://datatracker.ietf.org/doc/html/rfc7523)) at `/oauth2/token`.
- `agent_auth.skill` — the URL of this document.
- `agent_auth.identity_endpoint` — where you POST to register (Step 3).
- `agent_auth.claim_endpoint` — where you POST the claim invite and OTP (Step 4).
- `agent_auth.revocation_uri` — where the provider POSTs a [logout token](https://openid.net/specs/openid-connect-backchannel-1_0.html) to notify the service of upstream identity events.
- `agent_auth.identity_types_supported` — which registration methods this service accepts. Pick yours from Step 2.
- `agent_auth.identity_assertion.assertion_types_supported` — which assertion types this service accepts (ID-JAG, verified email, etc.).
- `agent_auth.events_supported` — event schemas this service can ingest (currently revocation). Informational; you don't act on these directly.

## Step 2 — Pick a method

Use this decision tree:

1. **You have a session tied to a user identity and can exchange it for an ID-JAG, audience-bound to this service** → [identity_assertion + id-jag](#identity_assertion--id-jag).
2. **You have only the user's email** → [identity_assertion + email](#identity_assertion--email). Claim ceremony required.
3. **You have neither** → [anonymous](#anonymous). Claim ceremony optional; deferred until the user wants to take ownership.

Before sending: cross-check your choice against the `agent_auth` block. If the matching `*_supported` array doesn't list your method, this service won't accept that registration shape — pick another or stop.

## Step 3 — Register

Before sending an `identity_assertion` (either variant), surface the service's `resource_name` and `resource_logo_uri` (from Step 1a) and the scope set you'll be acting under, and confirm with the user. This is the user's only consent gate before their identity is asserted to the service. Skip this for `anonymous` — there is no user identity to assert.

### identity_assertion + id-jag

Before minting the ID-JAG, confirm your provider is on this service's trust list (publishing format is service-specific — check the AS metadata or service docs). If it isn't, fall back to `identity_assertion + email` or `anonymous`.

Mint the assertion with:

- `aud` = the `resource` from the PRM
- `iss` = your provider's issuer URL (must be on the trust list above)
- `email_verified: true` OR `phone_number_verified: true`
- Fresh `jti`
- Near-term `exp` (~5 minutes)

```http
POST /agent/identity
Content-Type: application/json

{
  "type": "identity_assertion",
  "assertion_type": "urn:ietf:params:oauth:token-type:id-jag",
  "assertion": "<your ID-JAG JWT>"
}
```

Response (200):

```json
{
  "registration_id": "reg_...",
  "registration_type": "agent-provider",
  "identity_assertion": "<service-signed JWT>",
  "assertion_expires": "2026-05-04T13:00:00.000Z",
  "scopes": ["api.read", "api.write"]
}
```

Keep `identity_assertion` and go to [Step 5](#step-5--exchange-the-assertion).

### identity_assertion + email

```http
POST /agent/identity
Content-Type: application/json

{
  "type": "identity_assertion",
  "assertion_type": "verified_email",
  "assertion": "user@example.com"
}
```

Response (200):

```json
{
  "registration_id": "reg_...",
  "registration_type": "email-verification",
  "claim_url": "https://auth.service.example.com/agent/identity/claim",
  "claim_token": "clm_...",
  "claim_token_expires": "2026-05-21T17:31:25.994Z",
  "post_claim_scopes": ["api.read", "api.write"]
}
```

No `identity_assertion` yet — the service has already emailed the user, and the assertion is minted on claim completion. Keep `claim_token` and go to [Step 4](#step-4--claim-ceremony). `claim_token` is returned exactly once — hold it in memory for the duration of the ceremony; do not persist it past Step 4.

### anonymous

```http
POST /agent/identity
Content-Type: application/json

{ "type": "anonymous" }
```

Response (200):

```json
{
  "registration_id": "reg_...",
  "registration_type": "anonymous",
  "identity_assertion": "<service-signed JWT>",
  "assertion_expires": "2026-05-04T13:00:00.000Z",
  "pre_claim_scopes": ["api.read"],
  "claim_url": "https://auth.service.example.com/agent/identity/claim",
  "claim_token": "clm_...",
  "claim_token_expires": "2026-05-21T17:26:32.915Z",
  "post_claim_scopes": ["api.read", "api.write"]
}
```

The `identity_assertion` exchanges at `/oauth2/token` for an access_token with `pre_claim_scopes` immediately. If you also want a human to take ownership and unlock `post_claim_scopes`, go to [Step 4](#step-4--claim-ceremony). Otherwise skip to [Step 5](#step-5--exchange-the-assertion). `claim_token` is returned exactly once — hold it in memory for the duration of the ceremony; do not persist it past Step 4.

## Step 4 — Claim ceremony

The end goal: get the user to read a 6-digit OTP back to you.

### 4a. Trigger the claim email (anonymous only)

Skip this for `email` registrations — the email was sent during Step 3.

```http
POST /agent/identity/claim
Content-Type: application/json

{
  "claim_token": "clm_...",
  "email": "user@example.com"
}
```

Response (200):

```json
{
  "registration_id": "reg_...",
  "claim_attempt_id": "...",
  "status": "initiated",
  "expires_at": "..."
}
```

### 4b. Wait for the user's OTP

The user receives an email, clicks the link, sees a 6-digit OTP, reads it back to you. Surface this in your agent UI:

- Default ask: "Check your email and tell me the 6-digit code."
- If the user pastes the URL back instead of the code: "Open the link in your browser — the page will show a 6-digit code. Read it back to me."
- If the code is rejected: "That code didn't work — re-read it carefully, or open the email link again for a fresh one."

### 4c. Submit the OTP

```http
POST /agent/identity/claim/complete
Content-Type: application/json

{
  "claim_token": "clm_...",
  "otp": "123456"
}
```

Response on success (anonymous):

```json
{ "registration_id": "reg_...", "status": "claimed" }
```

Re-run [Step 5](#step-5--exchange-the-assertion) with the same `identity_assertion` you already hold — the resulting access_token now carries the post-claim scope set.

Response on success (email-verification):

```json
{
  "registration_id": "reg_...",
  "status": "claimed",
  "identity_assertion": "<service-signed JWT>",
  "assertion_expires": "..."
}
```

Keep the new `identity_assertion` and go to [Step 5](#step-5--exchange-the-assertion).

## Step 5 — Exchange the assertion

POST the `identity_assertion` to the AS metadata's `token_endpoint` with the [RFC 7523](https://datatracker.ietf.org/doc/html/rfc7523) JWT-bearer grant. The `resource` parameter is optional but recommended — it pins the access_token to the API you're calling.

```http
POST /oauth2/token
Content-Type: application/x-www-form-urlencoded

grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer
&assertion=<identity_assertion>
&resource=https://api.service.example.com/
```

Response (200):

```json
{
  "access_token": "<token>",
  "token_type": "Bearer",
  "expires_in": 3600,
  "scope": "api.read api.write"
}
```

Extract `access_token` and go to [Step 6](#step-6--use-the-access_token). The same `identity_assertion` can be re-used to mint additional access_tokens until it expires.

If `/oauth2/token` returns `invalid_grant`, your `identity_assertion` is expired or revoked. Restart the flow at [Step 3](#step-3--register) to mint a fresh identity assertion.

## Step 6 — Use the access_token

Present the `access_token` as a bearer token:

```http
GET /api/some-resource
Authorization: Bearer <access_token>
```

**Refresh.** When the access_token expires (`expires_in` seconds after issuance), re-call [Step 5](#step-5--exchange-the-assertion) with the same `identity_assertion`. When the identity assertion itself expires or `/oauth2/token` returns `invalid_grant`, restart at [Step 3](#step-3--register). There is no OAuth refresh_token in this flow — the two-step pattern replaces it.

If you get a 401 on a previously-working access_token: try [Step 5](#step-5--exchange-the-assertion) once with the current assertion. If that also fails, discard the expired identity assertion, and restart at [Step 1](#step-1--discover).

Full API reference: `https://docs.service.example.com/`.

## Errors

Errors at `/agent/identity` and `/agent/identity/claim/*` use profile-specific codes (the registration ceremonies have no OAuth analog). Errors at `/oauth2/token` use OAuth-standard vocabulary per [RFC 6749](https://datatracker.ietf.org/doc/html/rfc6749) / [RFC 7523](https://datatracker.ietf.org/doc/html/rfc7523).

| Code                         | Where                            | What to do                                                                                                                                                             |
| ---------------------------- | -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `anonymous_not_enabled`      | `/agent/identity`                | This service doesn't accept anonymous. Pick another method from Step 2.                                                                                                |
| `verified_email_not_enabled` | `/agent/identity`                | Email verification disabled here. Pick another method.                                                                                                                 |
| `issuer_not_enabled`         | `/agent/identity`                | Provider not on this service's trust list. Pick another method.                                                                                                        |
| `invalid_request`            | `/agent/identity`                | Body shape, missing claims, ID-JAG signature/`jti`/`aud` problems, or unverified identity. Fix the input (mint a fresh ID-JAG if signature/`jti`/`aud`/`exp`-related). |
| `invalid_claim_token`        | `/agent/identity/claim/complete` | `claim_token` wrong or expired. Restart at Step 3.                                                                                                                     |
| `otp_invalid`                | `/agent/identity/claim/complete` | OTP mismatch. Ask the user to re-read the code.                                                                                                                        |
| `otp_expired`                | `/agent/identity/claim/complete` | OTP window passed. Re-trigger the claim email (Step 4a) or restart at Step 3.                                                                                          |
| `claim_expired`              | `/agent/identity/claim/complete` | The whole registration expired. Restart at Step 3.                                                                                                                     |
| `previously_claimed`         | `/agent/identity/claim/complete` | Someone already finished this claim. Restart at Step 3 if you need a fresh assertion.                                                                                  |
| `invalid_grant`              | `/oauth2/token`                  | Assertion expired, revoked, replayed, or otherwise failed verification. Restart at [Step 3](#step-3--register) to mint a fresh one.                                    |
| `invalid_client`             | `/oauth2/token`                  | `client_id` not recognized. Re-read AS metadata.                                                                                                                       |
| `unsupported_grant_type`     | `/oauth2/token`                  | `grant_type` must be `urn:ietf:params:oauth:grant-type:jwt-bearer`.                                                                                                    |
| `rate_limited` (429)         | any                              | Back off and retry.                                                                                                                                                    |

Retry policy:

- 5xx → exponential backoff, retry the same request.
- 4xx → do not retry the same payload; act on the table above.
- 401 on a previously-working access_token → retry [Step 5](#step-5--exchange-the-assertion) once with the current assertion. If that fails, restart at [Step 1](#step-1--discover).

## Revocation

Two independent layers can kill what you're holding:

- **Credential layer ([RFC 7009](https://datatracker.ietf.org/doc/html/rfc7009), `revocation_endpoint`)** — agent-callable. POST `token=<access_token>&token_type_hint=access_token` (form-encoded) to the top-level `revocation_endpoint` to kill one access_token. 200 on success, idempotent. Your `identity_assertion` is intact; re-run [Step 5](#step-5--exchange-the-assertion) to mint a fresh access_token.
- **Registration layer (`agent_auth.revocation_uri`)** — provider-driven. The provider that minted your ID-JAG can POST a [logout token](https://openid.net/specs/openid-connect-backchannel-1_0.html) (`Content-Type: application/logout+jwt`) to this service's `revocation_uri`. The service invalidates the identity assertion and every access_token derived from it. You don't call this; you discover it the next time `/oauth2/token` returns `invalid_grant` — restart at [Step 3](#step-3--register).

On a 401 for a previously-working access_token: try [Step 5](#step-5--exchange-the-assertion) once. If `/oauth2/token` succeeds, the credential was revoked at the credential layer and your fresh access_token works. If `/oauth2/token` returns `invalid_grant`, the registration was killed at the registration layer — restart at [Step 3](#step-3--register).
