# Single sign-on

sip-source supports single sign-on with any OpenID Connect provider, such as Microsoft Entra ID,
Google, Keycloak, Okta or Authentik. It is off by default. Local password accounts keep working
alongside it unless you turn them off.

## Set it up

1. Register an application (client) with your provider, of the web or confidential type.
2. Add the redirect URL `https://YOUR-SIP-SOURCE/api/v1/auth/sso/callback`. The exact URL is shown
   under **Admin → Settings → Single sign-on**.
3. In sip-source, under **Admin → Settings → Single sign-on**, enter the **Issuer URL**, **Client
   ID** and **Client secret**, then press **Check provider**.
4. Turn on **Enable single sign-on**. The sign-in page shows a "Sign in with …" button.

Every setting can also be set with an environment variable, for example:

```bash
SS_SSO_ENABLED=true
SS_SSO_ISSUER=https://login.microsoftonline.com/TENANT-ID/v2.0
SS_SSO_CLIENT_ID=00000000-0000-0000-0000-000000000000
SS_SSO_CLIENT_SECRET_FILE=/run/secrets/sso_client_secret
```

Set `SS_SECRET_KEY` so that a client secret entered in the UI is stored encrypted.

## Accounts and roles

- An account is matched to the provider's user by issuer and subject, never by name, so single
  sign-on can't take over an existing password account with the same user name.
- With **Create accounts on first sign-in** on, a new user gets the **Role for new accounts**.
  With it off, an administrator creates the account first (Admin → Users, "Signs in with single
  sign-on only"), and it is linked on the user's first sign-in.
- **Role mapping** sets the role from the user's groups at every sign-in, for example
  `voice-admins=Administrator,noc=Analyst`. The groups are read from the **Group claim**
  (`groups` by default). Your provider may need to be told to include it in the ID token.
- With **Allow password sign-in** off, only accounts that can manage users can still sign in with
  a password, so the system stays reachable if the provider is down.

## Behind a reverse proxy

The redirect URL is built from the address the browser used. If your proxy changes the `Host`
header, set **Redirect URL** (`SS_SSO_REDIRECT_URL`) to the public address.

## What is checked

The authorization code flow with PKCE is used. The sign-in state is single use, bound to the
browser and valid for ten minutes. The ID token's signature is checked against the provider's
published keys, along with its issuer, audience, expiry and nonce. The provider must be reached
over HTTPS.
