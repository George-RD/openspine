# Gmail selected-thread drafting setup

This guide configures Lyra's current Gmail workflow for local or self-hosted use. The owner selects one Gmail thread by ID. OpenSpine verifies and scopes the request, Lyra prepares a reply, and the owner approves the exact text in Telegram before Gmail receives a draft. Email sending remains denied by runtime policy.

## 1. Create a Google Cloud OAuth client

1. In the [Google Cloud Console](https://console.cloud.google.com/), create or reuse a project and enable the **Gmail API**.
2. Configure the OAuth consent screen. **Testing mode is sufficient** for a single-owner development deployment. You do not need to publish the app or pass Google's verification review when the only user is the owner account.
3. Create an OAuth 2.0 **Desktop app** client. Note the `client_id` and `client_secret`.
4. Grant `gmail.readonly` to read the selected thread and `gmail.compose` to create the approved draft. Never grant `gmail.send`. Lyra has no send authority.

## 2. Obtain a refresh token

The kernel is a headless process. It does not run an interactive OAuth consent flow. A human completes Google's consent screen once and gives the kernel the resulting long-lived refresh token.

Any standard OAuth 2.0 authorization-code walkthrough for a Desktop app client works. Google's [OAuth 2.0 Playground](https://developers.google.com/oauthplayground/) can use your own client ID and secret with the two scopes above and return a refresh token.

### Docker Compose

Compose passes secrets from `.env` into the kernel container. Copy `.env.example` to `.env`, then fill these existing entries:

```dotenv
OPENSPINE_GMAIL_CLIENT_SECRET=your-client-secret
OPENSPINE_GMAIL_REFRESH_TOKEN=your-refresh-token
```

Do not rely on host-shell `export` commands for the Compose path. `compose.yaml` reads the values from `.env`.

### Bare metal

When running the kernel directly, export the same values into the process environment:

```sh
export OPENSPINE_GMAIL_CLIENT_SECRET="your-client-secret"
export OPENSPINE_GMAIL_REFRESH_TOKEN="your-refresh-token"
```

Do not put the literal client secret or refresh token in `openspine.yaml` or in the repository.

## 3. Add the `gmail` block to `openspine.yaml`

Add this block to the minimal configuration from [`docs/telegram-setup.md`](telegram-setup.md):

```yaml
gmail:
  client_id: "your-client-id.apps.googleusercontent.com"
  client_secret_env: OPENSPINE_GMAIL_CLIENT_SECRET
  refresh_token_env: OPENSPINE_GMAIL_REFRESH_TOKEN
  mailbox_address: "you@example.com"
```

`mailbox_address` is the owner's Gmail address. OpenSpine uses it to avoid replying to the owner's own message when it identifies the other participant in a thread.

The kernel can start without the `gmail` block, but `/draft` then replies that Gmail is not configured.

## 4. Select a thread with `/draft <thread_id>`

The kernel, rather than the agent shell, recognises the thread-selection command:

```text
/draft <gmail_thread_id>
```

Open a thread in Gmail and copy the trailing thread ID from its web URL. Send `/draft <that_id>` to the Telegram bot from the configured owner account.

OpenSpine then follows this path:

1. The kernel verifies that the thread exists before minting any task authority.
2. It creates a short-lived, single-use selection token bound to that thread and the requesting Telegram chat.
3. The contained agent reads only the selected, attachment-free thread.
4. The model gateway presents the email as untrusted data while Lyra prepares a reply.
5. Telegram shows the proposed draft and binds the approval to the exact text digest.
6. After owner approval, OpenSpine verifies the digest and creates the draft in Gmail.
7. `email.send` remains denied regardless of the task grant or approval state.

A thread browser or natural-language picker is still future work. The current alpha requires the explicit Gmail thread ID.

## 5. Containment requirements

Use `sandbox.driver: docker` for the Gmail workflow. Before starting Compose, build the task-worker image expected by `openspine.docker.example.yaml`:

```sh
docker build --file Dockerfile.shell --tag openspine-shell:latest .
docker compose up --build
```

`compose.yaml` mounts `artifacts/lyra` read-only into the kernel and the Docker example points `lyra_dir` at that mount.

The process driver is a development shortcut. Under `sandbox.driver: process`, `/draft` is refused unless the configuration explicitly sets:

```yaml
unsafe_allow_uncontained_private_data: true
```

Use that flag only in an isolated development environment. Omitting it is the safe default and produces a `route.refused_uncontained` audit row rather than running the private-data task without containment.
