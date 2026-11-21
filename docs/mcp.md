# Letting AI agents use MailCraft (MCP)

An AI app that speaks the Model Context Protocol (MCP), such as Claude Code or Claude Desktop,
can drive a running MailCraft: read and search mail and, once you let it act, file it, reply,
send, and anything else a command does. It works in MailCraft's window, and what it does shows
there.

## Turn it on

Agents get nothing until you turn on **File › Options › Safety › Let AI agents control
MailCraft**. Until then, MailCraft answers every request with that instruction. The switch is
saved with the other options. It can also be set from a script with
`--cmd 'options.agents={"on":true}'`.

Let in, an agent only reads: it opens folders and messages, lists and reads mail, and searches
mail, calendars and contacts. To let it also send, reply, delete, move mail and change settings,
turn on **Let AI agents send, delete and change settings** under it as well
(`--cmd 'options.agentsAct={"on":true}'`). An agent can't turn either on itself. A message an
agent opens is marked read as your reading options say, as when you open it.

## Connect an agent

The agent's app starts `mailcraft mcp`. That command opens no window: it carries the agent's
messages to the MailCraft that is running on the same data folder, over standard input and
output. Start MailCraft first.

| Where MailCraft is | The command to give the agent's app |
|---|---|
| Built from source | `target/release/mailcraft mcp` |
| macOS | `/Applications/MailCraft.app/Contents/MacOS/MailCraft mcp` |
| Windows | `"C:\Program Files\MailCraft\mailcraft.exe" mcp` |
| Linux (deb, rpm) | `/usr/bin/mailcraft mcp` |

- Claude Code: `claude mcp add mailcraft -- /usr/bin/mailcraft mcp` (with your path from above).
- Claude Desktop: add the command under `mcpServers` in its configuration file, with `mcp` as its
  argument.
- If MailCraft runs with `--data DIR` or `--sample`, give `mailcraft mcp` the same option. A
  `mailcraft mcp` with another data folder doesn't reach that window.

If MailCraft isn't running, or is a version from before agents could connect, the agent is told
so on its first request.

## What the agent can do

MailCraft offers two tools:

- **`list_commands`** lists MailCraft's commands, the same ones the ribbon, the shortcuts,
  `--cmd` and `mailcraft-cli run` use. Each has an id, a label, where it is in the window and its
  parameters, and says why when it can't run now. A filter narrows the list, for example `mail.`
  or `Calendar`.
- **`run_command`** runs one command by id with its parameters and returns its JSON answer. A
  command that fails returns why, and the window shows it as it would after a click. While
  agents only read, a command that would change something isn't run: it returns how to allow
  it, and `list_commands` marks it as one that can't run.

For example, `folder.open` and `list.messages` list a folder, `message.read` opens a message
(asked again when it answers `pending`: it is being downloaded), and `search.run` searches.

## What to keep in mind

- **Let act, an agent can do anything you can.** That includes sending mail, deleting it for
  good and changing settings, at once and without asking you. Most agent apps ask before each
  tool call unless you allow it: leave that on for `run_command`.
- **Mail can carry instructions meant for the agent.** A message can contain text that tries to
  get an agent to forward mail or delete it. Keep an eye on what an agent sends.
- **Only your user account can connect.** `mailcraft mcp` reaches the window through the socket
  MailCraft already uses for a second launch: it is in a folder only your account can open, and
  each connection must carry the key that MailCraft writes there when it starts. MailCraft opens
  no network port for agents.
- **Passwords stay out of reach.** No command reveals a password or a sign-in token.

## Limits

- At most four agents connect at once.
- A request that the window doesn't answer within a minute gets an error. A window that is
  minimized or hidden still answers, though more slowly (about a tenth of a second each), and
  shows what was done once it's on screen again.
- Tested on macOS. On Windows and Linux, `mailcraft mcp` uses the same code, but hasn't been
  tried with an agent's app.
