# Gmail selected-thread email preview setup (Phase 2)

How to configure the Gmail connector for `implement-selected-thread-email-preview-slice`
for local/dev use. This is the Phase 2 slice — the owner selects one Gmail
thread by id, the kernel drafts a reply via the model gateway, and the
owner sees a preview over Telegram. **No draft is ever created and no
email is ever sent by this slice** — draft creation is approval-required
(D-034) and its actual approval flow ships in
`implement-digest-bound-draft-approval` (Step 6).

## 1. Create a Google Cloud OAuth client

1. In the [Google Cloud Console](https://console.cloud.google.com/), create
   (or reuse) a project and enable the **Gmail API**.
2. Configure the OAuth consent screen. **Testing mode is sufficient** for a
   single-owner dev deployment — you do not need to publish the app or
   pass Google's verification review, since the only user is the owner
   account itself.
3. Create an OAuth 2.0 **Desktop app** client. Note the `client_id` and
   `client_secret`.
4. Grant these scopes during consent (D-029): `gmail.readonly` (this
   slice's only live call) and `gmail.compose` (needed once Step 6
   implements `email.create_draft` — requested together now so the owner
   completes consent once for both phases). **Never** `gmail.send` — Lyra
   never has send authority through any phase (D-004/D-015/PRD §22).

## 2. Obtain a refresh token

The kernel is a headless process — it does not run an interactive OAuth
consent flow itself (D-037). A human completes Google's consent screen
**once**, outside this codebase, and gives the kernel the resulting
long-lived refresh token via the environment. Any standard OAuth2
"authorization code" walkthrough for a Desktop-app client works (e.g.
Google's own
[OAuth 2.0 Playground](https://developers.google.com/oauthplayground/),
configured with your own client id/secret and the two scopes above,
produces a refresh token directly). Store it the same way as every other
secret in this repo:

```sh
export OPENSPINE_GMAIL_CLIENT_SECRET="your-client-secret"
export OPENSPINE_GMAIL_REFRESH_TOKEN="your-refresh-token"
```

**Do not put the client secret or refresh token in `openspine.yaml` or
anywhere in the repo** — same secret-intake shortcut as the Telegram bot
token and artifact key (see `docs/telegram-setup.md` §3, D-014's
deferral): plain environment variables for now, a richer secret-intake
flow is future work.

## 3. `openspine.yaml`'s `gmail` block

Add to the minimal config from `docs/telegram-setup.md` §4:

```yaml
gmail:
  client_id: "your-client-id.apps.googleusercontent.com"
  client_secret_env: OPENSPINE_GMAIL_CLIENT_SECRET
  refresh_token_env: OPENSPINE_GMAIL_REFRESH_TOKEN
```

Omitting the `gmail` block entirely is fine — the kernel starts normally
and the `/draft` command below replies "Gmail isn't configured on this
kernel yet" instead of failing to start (Phase 1's Telegram-only slice
must keep working with no Gmail connector at all).

## 4. Selecting a thread — the `/draft <thread_id>` command

PRD §15 requires that "the shell must not be trusted to provide target IDs
and claim the user selected them" — so the thread-selection trigger for
Phases 1-3 (D-036) is a structured command the **kernel itself** recognizes
directly in the Telegram poll loop, before any shell/agent ever sees the
message:

```
/draft <gmail_thread_id>
```

Find a thread's id from Gmail's own web UI: open the thread and look at the
URL, e.g. `https://mail.google.com/mail/u/0/#inbox/<thread_id>` — the
trailing hex string after the last `/` is the thread id. Send
`/draft <that_id>` to the same Telegram bot from
`docs/telegram-setup.md`, from the owner account.

What happens next (PRD §21.1):

1. The kernel verifies the thread exists via a live Gmail call, **before**
   minting anything.
2. The kernel mints a single-use, quickly-expiring selection token (PRD
   §15) and composes authority for `email_reply_drafter` as a new task,
   bound to the same Telegram chat that sent `/draft`.
3. The sandboxed shell reads the bounded, attachment-free thread content
   (`email.read_thread:selected_no_attachments`) using — and consuming —
   that token.
4. The thread content is drafted into a reply via the model gateway, with
   the raw email text wrapped as untrusted, non-authoritative data (PRD
   §13) — a prompt-injection attempt inside the email body cannot make the
   model take an unauthorized action.
5. The draft is previewed back to the owner over Telegram
   (`lyra.ui.preview`). **No draft is created in Gmail and no email is
   sent** — this slice's output is preview-only.

A real thread-browsing picker (list recent threads, "the one from Alex
about the invoice") is explicit future work, not built here — see D-036
for why a narrow, kernel-recognized command is this slice's real scope,
not a shortcut around it.

## 5. Unsafe dev shortcuts (do not carry into production)

Everything in `docs/telegram-setup.md` §5 applies unchanged. One addition
specific to this slice: `/draft` triggers an `external_communication`-lane
event (unlike Phase 1's `owner_control`-lane chat), so the D-025/O-003
containment guard actually engages here. Under `sandbox.driver: process`
you must also set `unsafe_allow_uncontained_private_data: true` in
`openspine.yaml` to exercise `/draft` at all — omitting it is the safe
default and produces a `route.refused_uncontained` audit row, not a bug.
Real deployments use `driver: docker`, where the guard is satisfied by the
container boundary itself and this flag stays `false`.
