# Architecture

This page explains how MailCraft's crates fit together and why they are split this way. It is for
contributors. The rules every change follows are in [`CLAUDE.md`](../CLAUDE.md); what each version
added is in the [changelog](../CHANGELOG.md).

## How a click becomes network work

On the desktop (`apps/mailcraft`, built on eframe), one user action travels through the layers
like this:

1. `ui-egui` (L6) turns the input into a command.
2. `engine::Session` (L4) runs the command and queues `Job`s.
3. `net::Pool` (L3) takes the jobs and talks to the server over IMAP, SMTP, JMAP, Microsoft Graph,
   EWS or CalDAV/CardDAV.
4. The pool writes the results into the local cache (`store`, L1) and sends `Event`s back to the
   session.
5. The UI draws from the session's state.

The app itself supplies the keychain, file dialogs, the system font and the clock.

In the browser (`apps/mailcraft-web`), the same `ui-egui` and engine run over JMAP. See
[The browser](#the-browser-appsmailcraft-web).

## Layers

| Layer | Crate | Depends on | Job |
|---|---|---|---|
| L0 | `mailcraft-model` | — | Pure data and algorithms. `Job` and `Event` live here, so `net` and `engine` agree without depending on each other |
| L1 | `mailcraft-mime` | model | Parse and build MIME (`mail-parser`, `mail-builder`); turn HTML into text (`html2text`, `html5ever`) |
| L1 | `mailcraft-store` | model | The local cache: one redb data file per account, or plain files plus JSON; atomic writes; the search index |
| L1 | `mailcraft-print` | — | Messages as PDF pages, laid out as they're read (tables, backgrounds, pictures, links), with an embedded TrueType font |
| L1 | `mailcraft-pst` | — | Outlook data files (MS-PST): read them (wasm-safe), add to them and write new ones |
| L1 | `mailcraft-smime` | — | S/MIME (RFC 8551) on RustCrypto, with keys behind a `PrivateKey` trait; wasm-safe |
| L1 | `mailcraft-pgp` | model | OpenPGP (RFC 9580) on rPGP: keys and their self-signatures, signing, encrypting, decrypting, verifying; Autocrypt headers; inline PGP blocks; wasm-safe |
| L2 | `mailcraft-keys` | smime | Keys on smart cards (PKCS #11), in the macOS Keychain and in the Windows certificate store; native only |
| L2 | `mailcraft-jmap` | model, mime, store | JMAP, async over a two-method `Http` trait; wasm-safe |
| L3 | `mailcraft-net` | model, mime, store, pst, smime, pgp, keys, jmap | All network and `.pst` I/O; native only; TLS through `rustls` |
| L4 | `mailcraft-engine` | model, mime, store, print, smime, pgp | The sans-I/O `Session` and the command registry; wasm-safe |
| L5 | `mailcraft-mobile` | engine | The phone apps' core: a [Crux](https://github.com/redbadger/crux) app over the `Session`, whose view model the Android and iOS shells draw; wasm-safe |
| L6 | `mailcraft-ui-egui` | engine | The egui interface; platform services behind traits; interface languages |

- `cargo xtask layers` enforces this table: `egui` appears only in L6, and network crates only in
  `net`.
- `cargo xtask wasm` builds every crate except `net` and `keys`, plus the web app, for `wasm32`.
- `jmap`'s `Http` trait is served by `net`'s HTTP client on the desktop and by `fetch` in the
  browser.
- `net` covers IMAP, POP3, SMTP, JMAP push, Microsoft Graph, EWS with NTLM (Windows sign-in over
  HTTP, and over IMAP, POP3 and SMTP when an account asks for it), CalDAV/CardDAV, ManageSieve,
  HTTP, DNS and OAuth. It also holds the log file (`logfile.rs`, `mailcraft.log` in the data
  folder), which the desktop app and the phone library both write.
- `keys` and the phone library `apps/mailcraft-mobile` are the crates with `unsafe`. Each denies
  it; in `keys` only `windows/cng.rs` and `macos/sec.rs` allow it, in the phone library only
  `android.rs` and `ios.rs` (the exported entry points, and the C strings iOS hands over).
- `.pst` files (Outlook data files) go through MailCraft's own crate, `pst`, written from
  Microsoft's published [MS-PST](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/)
  specification. It reads both formats (Unicode and the older ANSI), adds to files and makes new
  ones, and holds only what MailCraft uses.
- **Portable crates start no threads.** Slow window work, such as decoding pictures and video
  frames or parsing the open message, goes through `Services::background` to a *lane*: a named
  queue of background jobs. The desktop runs one thread per lane
  (`apps/mailcraft/src/desktop/lanes.rs`) and runs each job under `catch_unwind`. The browser and
  the tests run a lane one step per frame.

## Source layout

Code is split by topic. `foo.rs` sits beside a `foo/` folder and declares its pieces; there is no
`mod.rs`. A big type keeps one `impl`, continued across files. The size limits are in `CLAUDE.md`,
and `cargo xtask sizes` enforces them.

| Where | What |
|---|---|
| `engine/src/session.rs` + `session/` | `Session`'s state, with its `impl` continued per area (`opening`, `changing`, `writing`, `searching`, `accounts`, `organize/`…) |
| `engine/src/cmd.rs` + `cmd/` | The command registry; each area returns its own commands |
| `ui-egui/src/app.rs` + `app/` | `MailApp`, the window |
| `ui-egui/src/panes/`, `dialogs/`, `reading/`, `rich_edit/`, `look/` | Panes; dialogs; the reading pane and compose; the editor; theme and icons |
| `net/src/protocols/`, `exchange/`, `data_files/`, `fakes/`, `worker/` | Protocols; Graph, EWS and NTLM; `.pst` files; test servers; the worker pool |

## Transports

- Mail syncs over IMAP with SMTP, POP, JMAP, Microsoft Graph or EWS. All of them use the same
  `Job`s and `Event`s.
- JMAP, Graph and EWS name messages with strings. `jmap::State` maps those to IMAP-style numbers,
  so the engine and the local cache only ever see IMAP-style keys.
- Calendars and contacts sync over CalDAV and CardDAV. Automatic replies use ManageSieve.
- Some accounts have no server, such as the sample mailbox or an opened `.pst` file. These are
  *local* accounts: the pool applies their changes straight to the local cache.

## MailCraft's own AI

- **Where it lives.** The session's `ai` module (`engine/src/session/ai.rs`) keeps what the
  person set up (`Prefs::ai`: model profiles, which one does each kind of work, how far each
  account's mail may go, the local-only switch), words each question and records it. It queues
  `Job::Ai`; `net/src/worker/ai.rs` reads the profile's key from the keychain and asks on a
  thread of its own, at most four at once; `net/src/protocols/ai.rs` speaks the three wire
  formats, with the tools a model may call, and reads an answer whole or streamed as it's
  written (`ai/streamed.rs`: the server-sent events OpenAI's and Anthropic's formats stream, or
  Ollama's JSON lines, its growth sent on as `AiDone::Part` every 80 ms at most). A question
  whose server was busy (429), failed on its side (5xx), or timed out or cut the connection is
  asked once more after five seconds (`ai/again.rs`), unless some of its answer came; still
  failing, it goes to the profile's fallback, then to the person (`assistant.retry`, Try again).
- **Three formats cover the servers.** OpenAI's Chat Completions is spoken by OpenAI, xAI, Mistral,
  OpenRouter, Groq, DeepSeek, Gemini's compatible address and the local servers (Ollama, LM
  Studio, llama.cpp); Anthropic's Messages is the second. Ollama's own (`/api/chat`) is the
  third, because only there is the server told how much the model takes in (`num_ctx`, from the
  profile's context): through its OpenAI-compatible address Ollama keeps to the server's own
  (4096 tokens in some versions, unless set) and can drop the start of a longer question without
  a word. It's asked to refuse one still too long (`truncate` and `shift` off) rather than cut it.
- **Why not the `genai` crate.** It is async on tokio and reqwest, with TLS of its own: it would
  bring a second HTTP stack that bypasses MailCraft's certificate rules (`net/src/stream`), and a
  runtime the worker threads don't need. The three formats, tools and streaming are under a
  thousand lines over `net::http`.
- **MCP servers** the person adds, whose tools MailCraft's AI calls, are reached by
  `net/src/protocols/mcp.rs`: a program MailCraft starts (JSON-RPC over its input and output, a
  message a line, each way on a thread of its own so a wait ends) or a server on the web
  (Streamable HTTP: each message posted, its answer JSON or server-sent events; https, or http
  to this computer). `Job::Mcp` lists a server's tools, calls one or closes its connection;
  `net/src/worker/mcp.rs` keeps one connection for each server (a program left running between
  calls, ended as its connection goes), does at most four pieces of work at once, and reads a
  server's key from the keychain. A server on the web that asks for a sign-in is signed in to
  by `mcp/sign_in.rs` (MCP's authorization): its 401's challenge names its metadata (RFC
  9728), which names its authorization server, whose metadata (RFC 8414) must name itself and
  take PKCE's S256; MailCraft registers there for its redirect (RFC 7591, no secret of its
  own) and signs in through `oauth.rs`, the tokens asked for the server alone (`resource`,
  RFC 8707) and the redirect's `iss` checked (RFC 9207). The keychain keeps the tokens under the
  server's key; the server's settings keep how to renew them (`McpOAuth`), and the pool renews
  the access token as it runs out, keeping the refresh token it turns over to. Written by
  hand, as the model formats are: the `rmcp` crate is async on tokio. The engine keeps the servers (`AiPrefs::mcp`, `session/ai/servers.rs`) and
  what each listed (`mcp-tools.json`, read when first wanted), so tools are found without
  starting every server; `mcpServer.*` adds, changes, turns on and off, lists again and removes
  them, and `mcpServer.signIn` signs in to one. Adding or changing a server, or turning one on,
  starts a program, and signing in uses the person's own account: `mcp::PERSON_ONLY` keeps
  those commands from every outside agent, even one let to act. A script calls them
  (`script/mcp.rs`): `find_tools` searches what was listed, and `mcp(server, tool, args)` calls a
  tool that says it changes nothing (`readOnlyHint`), the script waiting for its answer as for
  a message (`Wait::Mcp`); any other is proposed, as a script's changes are, and called once
  approved (`mcp_approve`), its card waiting for the answer and the chat going on after it. It
  calls only a server every account the model reads lets its mail go to
  (`AiPrefs::allows_server`: one elsewhere than this computer or its network only where mail
  may go to any model). Each call is recorded as a question is: `MCP tool`, the server, the
  tool and the sizes.
- **The record.** Each question is recorded before it goes (`ai-log.json`, saved as the jobs are
  taken): when, what for, which profile and server, whose mail, and its sizes; never what it
  said. The newest 500 are kept.
- **Keys stay out of errors.** What a server says is quoted with the key blanked out, before it's
  cut short; an error of the network layer that quotes a server's words (a status line it
  couldn't read) and holds the key, or the start of it, says less instead.
- **The assistant** (`session/ai/assistant/`) chats with a model that has tools. Those that read
  run at once; finding goes through `search_aside` (`session/searching/aside.rs`: the window's
  matching over the summaries, each account's index and the sources downloaded, without its list,
  selection or search). A change is a proposal: made only once the person approves it, checked
  again then, on the messages it names (never the selection), and undone with Undo; a message it
  writes opens to be sent. One proposed beside a find or a read is put off (`RunState::PutOff`,
  no card), for the model to propose again with the rest, so a request's changes are approved
  together. An event, a contact, a task or a note it adds or changes
  (`session/ai/assistant/pim_changes/`) is made as the window's forms make one
  (`cmd::event_from`, `card_from`, `task_from`), in an account its model may read; what it
  changes or deletes is named as the chat found it. It invites and tells no one: a meeting with
  others is left to the calendar (but for its reminder, the person's own), as is the time of an
  event that repeats. A model asked about a chat must be allowed every account whose mail
  is in it (`AiPrefs::allows_all`), its fallbacks too. A chat that outgrows its profile's
  context (a token for every three bytes and a few for each message, raised up to threefold
  when the server counted more of the last question, `Chat::counted`) loses what tools read
  early on, what they found still named, then its oldest turns, what those asked, found and
  decided said in a ledger made from the chat (`ledger.rs`). At most 24 steps for one thing
  the person says. Stopped as it's written, what came of an answer is kept
  (`ChatTurn::stopped`) until it's asked again.
- **Scripts** (code mode, `session/ai/assistant/script/`): a model the person lets
  (`AiProfile::scripts`) gets `run_script`, a script in Rhai (MIT or Apache-2.0, through a fork
  pinned in `[patch.crates-io]`: `compact_str` in place of `smartstring`, and a limit on how deep
  arrays and maps nest) whose functions are the tools that find and read, giving data rather than
  text. Each call is the tool's own, with its checks, and shows under the script's card
  (`ToolRun::part_of`). A run is replayed: the script runs from its start against what its calls
  were answered so far; at the first call not answered it stops (a non-catchable error), the call
  is made, and it runs again, so a read of a message still to download pauses it
  (`Assistant::scripts`) until `Event::Body`. Only the run that ends is heard. Bounded in calls,
  operations, sizes, variables, and depth of calls, expressions and data (measured on a 1 MiB
  stack); no `eval`, modules, clock or custom syntax; each run in `catch_unwind`. A change it
  calls for is proposed (at most 20), the script told so: the run keeps what it was called with
  (`ToolRun::input`), kept with the chat, to make it from once approved after the script
  ended (a script that fails withdraws them), and the model is told what became of each with
  what the script came to. `ask(question, text)` (`script/asking.rs`) asks the chat's model a
  question of its own, without tools or the rest of the chat, its text cut to what the model
  takes in, recorded as `assistant script`; at most 20 a script. The script waits for the
  answer as for a download, and a chat stopped or deleted lets the question go. The model's
  call ids never hold a `#`; the script's calls are `{script}#{n}`. Kept scripts
  (`script/kept.rs`, `ai-scripts.json`, starting with MailCraft's own) are each a function's
  body taking the map `params`: every script gets them as functions (compiled apart, their
  functions alone merged in, so nothing outside one runs), and a model that may run scripts as
  tools of their own, run as `run_script` is. Keeping one is a change (`keep_script`, checked:
  a free name, code that compiles) made once the person approves its card; `script.*` lists,
  changes, turns off and deletes them, as Options' Scripts card does; `script.run`
  (`script/agents.rs`) runs one at once for an outside agent or the CLI, reading every account
  as their commands do (`AiProfile::agent`) and changing nothing.
- **Chats stay on this computer**: `assistant.json` lists them, each in `assistant-<id>.json`
  (read when first wanted, its file removed when it's deleted); never in the record or the log.
  They're written on the store's thread (`store/src/docs_later.rs`, read back as handed over,
  waited for at closing); a card keeps the first thousand characters of what a tool gave, the
  turn keeps it whole; a chat takes no more past 8 MB.
- **The pane** (`ui-egui/src/panes/assistant/`) shows the chat from the session, never a copy:
  what the person says and decides goes through `assistant.*` commands. An answer is Markdown
  (`egui_commonmark`) with each picture made a link, so nothing is fetched; its links open as a
  message's do. Text selected in a message is read as its runs are laid out, so its Ask AI
  button takes it a frame after it's pressed (`reading/text_selection/asking.rs`).
- **Summaries** (`session/ai/summary.rs`, `mail.summarize`): a conversation as the reading pane
  shows it (or worked out from the list, with the person's own replies), its newest messages
  while 48 KB hold them, asked of the chat model as one streamed question with no tools, its
  purpose `summary` in the record. The answer is kept by the message it was asked from (the
  newest 16), and one asked again or let go drops the answer still on its way. Its card is
  `ui-egui/src/panes/assistant/summary.rs`, at the top of the reading pane.
- **Links** (`session/ai/assistant/links.rs`): the tools name each message by `m` and ten hex
  digits of its id's FNV-1a hash, since an id holds its folder's name (spaces and all) and a
  Markdown link can't; events (`e`, `find_events`), people (`p`, `find_people`), tasks (`t`)
  and notes (`n`) the same way (`session/ai/assistant/pim.rs`), of what finds them again. The
  model links what it names as `[subject](mailcraft:m…)`, and `assistant.open` goes to it: a
  message opened in its folder; the calendar on an event's day, People, Tasks or Notes, with
  what the window opens (the event, the person's card, the task, the note). A name is looked up
  only among what the chat has found or seen, and names are kept in saved chats, so how one is
  made never changes.
- **Not yet:** embeddings (for search), pictures, a monthly budget, the browser and the phones;
  the assistant's tools for rules, and inviting people or telling them of a change.
  A question waits up to five minutes for its answer to start.

## The session

`Session` (`crates/engine`) holds everything the window shows:

- the accounts;
- the view: one folder, or one role across accounts, such as All Inboxes;
- the filter and the search;
- the message list and the selection;
- the open message (`Reading`) and the draft (`Compose`);
- preferences and notices.

How it works:

- **Everything is a command.** `run(id, params)` looks the command up in the registry (`cmd.rs`),
  checks that it is `enabled`, and turns an escaped panic into an error. Commands never open
  dialogs; the UI asks first, then runs the command.
- **The session does no I/O.** `take_jobs()` hands work to the network layer, and `apply(event)`
  folds the results in. The session has no clock, no network and no threads.
- **Changes show at once.** `apply_op` updates the list in memory. The worker then applies the
  change to the local cache and the server and reports `FolderChanged`. That reloads the list from
  the local cache, which is the single source of truth. A change's *echo* (the `FolderChanged`
  that reports the session's own change) doesn't reload (`session/mailboxes/echo.rs`).
- **The message list** is indexed as it is built (`ListNav`). Flag and category changes are
  patched into it in place (`patch_list`). The proptest `patched_list_equals_refresh` checks that
  a patched list always equals a full rebuild. Summaries are shared as `SharedSummary`
  (`Arc<Summary>`).
- **The open message** can be parsed on the reading lane (`session/opening/prepare.rs`). A command
  that acts on it finishes parsing the body first. The CLI and the tests always parse straight
  away.
- **Tasks** live in each account's `pim.json`; an on-premises Exchange account's also in its
  mailbox's Tasks folder, synced as its notes are (`net/src/exchange/ews_pim/tasks.rs`).
- **Sending:** `Session::send_block` says why an account can't send. `Session::deliver`, which
  every way of sending goes through, refuses to send from such an account.

## Storage

`crates/store` keeps the local cache.

- **Data files.** Each account's local cache is one crash-safe redb file, its *data file*, named
  `<address>.mailcraft`, in **Documents\MailCraft Files** (`packed.rs`). The account list and app
  state stay in the app folder (`%APPDATA%\MailCraft` on Windows). The sample mailbox uses plain
  files, and the browser keeps everything in memory.
- **An opened `.pst` file is never written.** It gets its own `.mailcraft` data file beside it,
  listed in `datafiles.json` (`beside.rs`). MailCraft writes `.pst` files only when it exports one.
- **Old folder layouts** are brought into data files when the store opens. This needs the store to
  itself, so it holds `<root>/.locks/folders.lock`.
- **Moving the data folder** (`location.rs`): a requested move runs at the next start, before
  anything opens the files.
- **Commits** (`packed/commit.rs`):
  - Writes from the window reach the disk at once.
  - Background writers, such as imports, compress off the file and commit in batches without
    waiting for the disk.
  - A data file is *sealed* (marked consistent on disk) after two quiet seconds, at the end of an
    import and on exit. After a crash, a sealed file reopens at once; an unsealed one gets one full
    check first.
- **Opening** one data file locks no others. The desktop opens them on a `data-open` thread while
  the window shows `BootCard`.
- **Health and compaction** (`packed/health.rs`, `compact.rs`, `shrink.rs`):
  - Health is measured without reading any mail.
  - Compaction runs in place when it fits the time allowed. Otherwise it copies the file, if the
    drive has room (`mailcraft_model::job::plan`).
  - The pool compacts on a `mail-compact` thread, never while an import runs.
  - The engine measures health after ten idle minutes. While a data file is being compacted,
    commands on its account are refused.
  - `dataFile.compact` compacts by hand. Compacting on close is off by default
    (`prefs.compactOnClose`).
- **Work kept off the window thread:**
  - On local accounts, read marks, flags and categories go to the `mail-marks` thread
    (`Store::mark_later`). Loading a folder waits for its pending marks (`Session::load_cached`
    defers it).
  - `ui.json` and `mailcraft.log` are written on threads of their own.

## Importing .pst files

- The session tracks imports in `Imports` and `imports.json` (`session/accounts/importing.rs`,
  `cmd/data_files.rs`). `account.inspectDataFile` looks inside a file before `account.importPst`
  imports it.
- **Resuming.** Each import keeps a note in the account's data file (`import.json`). The note is
  committed with each batch, at least every 10 seconds. An interrupted import resumes at the next
  start. If the source file changed, went missing or is locked, the import waits for the user.
- **While an import runs,** the account's folders can't be reorganised, and new Inbox mail isn't
  treated as new. The import pauses when free disk space drops below 1 GiB plus the data file's
  next growth (`Blocked::DiskFull`).
- **The importer** (`pst/` in `net`):
  1. A planner hands out numbered jobs.
  2. Lanes build items, within a byte budget.
  3. One writer applies the items in order.

  A panic in a lane costs only the item it was building. The `.pst` file is opened read-only.
  Sent Items is read last, so replies can find the messages they answer by Message-ID. Mail is
  indexed for search while it imports.
- **Stopping.** `pst::Stop` steps from pause to cancel to discard. Closing the app pauses an
  import.

## The browser (`apps/mailcraft-web`)

- The local cache is `Store::memory()`. It is loaded from IndexedDB at start, or from local
  storage if IndexedDB is missing. Each frame writes the changes (`Store::take_changes`) back.
- Settings go in local storage. Passwords never do.
- `WebBackend` runs each account's jobs in order over `fetch`. It keeps the JMAP push stream open
  and polls when push isn't available.
- Browser accounts are JMAP only (`Session::jmap_only`).

## The phones (`apps/mailcraft-mobile`)

- `crates/mobile` is a Crux app: `update` runs engine commands on the `Session` for each of the
  person's actions (`Action`, JSON) and asks for two effects, `Render` and `Engine` (the jobs
  the commands queued). `view` builds one `ViewModel` (JSON) from the commands' own answers
  (`account.list`, `list.messages`, `message.read`), kept in the model after each update.
- The native library (`apps/mailcraft-mobile/src`) runs the Crux core and `net`'s worker pool on
  one thread of their own (`driver.rs`). It does the `Engine` effects itself, so jobs and
  passwords never cross into Kotlin or Swift that way; the shells see only the view model.
- The shells talk to it through six calls (`start`, `send`, `poll`, `view`, `zone`, `background`), in JSON:
  JNI for Kotlin (`android.rs`), C for Swift (`ios.rs`). A shell polls from a thread of its own.
  No UniFFI or generated types: the view model is read as JSON on both sides.
- Passwords are kept by the phone's keystore (the Android Keystore, the iOS Keychain). They're
  handed to the library at start, and each one the pool keeps or drops comes back to the shell
  as a `secret` or `forget` message (`credentials.rs`), named as the desktop's keychain names
  them.
- The cache is the desktop's: a data file per account (`Store::packed`, then
  `open_data_files`), in `Data Files` inside the app's own folder.
- The view model is the drawer's too (`drawer.rs`): each account's folders in the desktop folder
  pane's order (`Session::pane_folders`), as a tree, the special folders at the top wherever the
  server keeps them, and what's collapsed (`view.collapse`, the desktop's own) left out. Avatars
  are initials and a hue from the address (`avatar.rs`), so both shells draw the same.
- Certificates: Android lists no system roots, so a server whose issuer Mozilla's roots don't
  know is asked of Android's trust store (`net/src/stream/trust/android.rs`, through
  `rustls-platform-verifier`; the phone library hands it the app's context first). The app's
  network security config trusts the system's CAs alone, never those a person, an app or a VPN
  installed (the owner's decision).
- Icons: Google's Material Symbols, kept in `assets/icons/material-symbols.json` (pinned upstream)
  and turned into each shell's code by `cargo xtask symbols`.
- The shells: `android/` (Kotlin, Jetpack Compose, Gradle; `build.sh` builds the Rust library
  with `cargo ndk` first) and `ios/` (SwiftUI; `build.sh` makes an XCFramework and the Xcode
  project with XcodeGen).

## Search

### Queries

- `model::search` parses the query language (`search/parse.rs`). Dates are resolved with a clock
  the caller passes in, because portable code can't read one.
- Words are folded on both sides before they are compared (`search/fold.rs`): lower case, accents
  removed, ß becomes ss. `cargo xtask fold` generates the folding table.
- `Query::eval` answers yes, no or unknown for a message.

### Running a search

- The session searches in small units (`session/searching/run.rs`: `start_search`,
  `search_step`).
- A message's summary answers questions about flags, dates, senders and subjects. The index
  answers questions about text.
- In the window, a search runs for 6 ms each frame. Results join the list by date as they are
  found.
- A search that waits for its answer (the CLI, the tests) reads mail that isn't indexed yet from
  its source. In the window, such mail may be missing from the results, and the list says so.

### The index

The index lives with the account's mail, under `<account>/search/v1/`
(`store/src/search.rs` + `search/`).

- It is a series of *generations*. Each holds immutable word segments plus each message's kept
  text.
- `meta.bin` names the current generation. It is written last, in one sealed transaction, so a
  crash leaves the previous generation in place.
- Each account has one index writer, even across processes. The writer holds a lease in
  `<root>/.locks/`.
- Every index file is decoded with checks, and the `search_index` fuzz target tests that decoding.
  A damaged index is set aside and rebuilt.
- **Keeping the index current:** `search::plan` compares each folder with the index and adds,
  removes or moves entries. Nothing hooks into each change.
- Building the index is `Job::Index`. It starts after 1.5 seconds of quiet. On the desktop it runs
  on a `mail-index` thread, never during an import; in the browser it runs in small steps.

## Conversations

`model::thread` groups messages into conversations with JWZ threading, the same method as IMAP
`THREAD=REFERENCES` (RFC 5256):

- A message's parent comes from its `References` and `In-Reply-To` headers, with loops removed.
- A message whose parent is missing is placed by its conversation index (`Thread-Index`).
- Replies that lost their references are grouped by base subject (the subject without "Re:" and
  similar prefixes).

Every message is in exactly one conversation. Everything sorts by `Summary::when`.

## Interface languages

- Interface text is written in English in the code and translated through `tl!` and `tf!` from
  `crates/ui-egui/src/i18n/*.tsv`. Command ids, the CLI and mail stay in English.
- The interface follows the system language unless the user picks one in Settings.
- Release builds can embed fallback fonts from
  [storytold/craft-fonts](https://github.com/storytold/craft-fonts) through `CRAFT_FONTS_DIR`. The
  fonts are never a Cargo dependency.
- egui and winit are the Craft forks (storytold/egui, storytold/winit), pinned in the root
  `Cargo.toml` under `[patch.crates-io]`. That egui lays out right-to-left text itself.

## The Craft apps' shared libraries and services

MailCraft uses what the other Craft apps share where it fits, so the suite's fixes reach it.

| Shared piece | Used | Why, or why not |
|---|---|---|
| `craft-atomic` (craft-libs) | yes: `crates/store/src/files/atomic.rs` | The cache's file writes: a new scratch file never written through one at its name, a plain flush where a Mac's network share refuses a full one, renames tried again while Windows holds a file |
| `craft-docpath` (craft-libs) | yes: `.pst` linked files | Whether a path a data file names is a local file: device names, Windows' file streams and `\\?\` forms refused |
| `craft-proof` (craft-libs) | yes: `crates/ui-egui/src/rich_edit/spelling.rs` | Spelling as you type (#174): the words it knows, its suggestions, the user's words. Built without `craft-i18n`'s data, so English is its built-in word list and other languages are the system's Hunspell dictionaries |
| craft-fonts, craft-common-actions | yes | The fonts in releases; CI, packaging lint and the FreeBSD and Windows ARM64 builds |
| Dependabot, `SECURITY.md` | yes | Weekly grouped dependency updates; private vulnerability reports |
| `craft-update` (craft-libs) | no | Built for signed suite releases: it refuses unsigned builds on Windows and macOS (an Apple team of the suite's), skips pre-releases, and wants an asset's version to equal its tag's. MailCraft's own updater (`docs/updates.md`) takes its ways of telling installs apart instead |
| `craft-quit` (craft-libs) | not yet | `apple_events::quit` and `MailApp::quit_instead_of_close` are current copies of it; the swap is macOS code, to be built and tried on a Mac |
| `craft-dialogs` (craft-libs) | not yet | File dialogs that don't block the window (a macOS crash with a blocking one); every file picker would become ask-then-answer, and no Craft app uses it yet |
| `craft-i18n` | no | Its catalogs are storytold's repository, for its apps; MailCraft's would move there by a PR to it. Its rule that a missing translation never fails is the opposite of MailCraft's |
| Flatpak OCI publishing (`flatpak-oci.sh`) | no | Publishes to storytold's registry from the shared release, which MailCraft's releases don't use; a Flatpak updates through the software installer |
| SonarCloud | no | Needs a SonarCloud project and token for ZIDAN44, and its coverage builds run on every push |
| Contributor credits (`contributors/`) | no | The generator is shared, but the About page credits are each app's own code, for one maintainer |
| Other craft-libs crates | no | Nothing to replace (audio, PDF, raster, fonts for documents), or a larger rewrite than they'd save (`craft-ui`, `craft-menu`, `craft-print`) |

## Crash safety

MailCraft must never crash; the rules are in `CLAUDE.md`. `Session::run` (a command),
`Session::apply` (an event), lanes and the importer's lanes catch escaped panics. A damaged cache file or index is set aside, never fatal.

## Tests

- `model`, `mime`, `store`: unit tests, including hostile input.
- `net`: in-memory IMAP, JMAP, Graph, EWS, CalDAV/CardDAV and ManageSieve servers (`fakes/`)
  drive accounts through the real pool. Local stubs stand in for SMTP, HTTP, DNS and OAuth.
  `serve_for_the_browser` (ignored by default) runs a JMAP server with CORS for the web app.
- `.pst` export and import round trips, including calendars, contacts, notes and tasks.
- Fuzz targets for every parser of network and disk input; see [Fuzzing](../fuzz/README.md).
- `engine`: commands against the sample mailbox and scripted events, plus every command with junk
  params.
- `ui-egui`: headless frames of every layout and dialog, and a check that every language catalog
  covers every interface string.
