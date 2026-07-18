# Butter TTS

A voice changer for Discord, as a single portable app for Windows and macOS.

You talk into your microphone. Butter TTS transcribes what you said and reads it back
into the voice channel in a synthetic voice. **Your real microphone audio never reaches
Discord** — the channel only ever hears the synthesised re-reading, and the recording of
your voice is thrown away as soon as it has been transcribed.

## Download

Grab the build for your platform from the [Releases page](https://github.com/voidhamsterofficial/butter-tts/releases):

- **Windows**: `butter-tts.exe`
- **macOS**: `butter-tts-macos.zip` — unzip it first

There is no installer. It is one file, and it keeps its settings and history next to
itself — put it in its own folder and it will stay tidy.

> Windows may warn that the app is unrecognised, because the exe is not code signed.
> Choose *More info* → *Run anyway*.
>
> macOS will refuse to open the binary with a plain double-click, because it is not
> notarised. Right-click it and choose *Open* instead, then confirm in the dialog that
> appears — you only need to do this once.

## Setting it up

1. **Create a Discord bot** in the [developer portal](https://discord.com/developers/applications),
   add a bot to it, and copy the token. Invite it to your server with permission to join
   and speak in voice channels.
2. **Get an OpenAI API key** with some credit on it. It pays for the transcription and the
   speech.
3. Open the app, go to **Settings**, paste both in, choose your microphone, and save.
4. Press **Wake up** on the Home page.
5. In Discord, sit in a voice channel and type `/join`. Then talk.

The **Docs** page inside the app explains the sliders, the history, and what to do when
something misbehaves.

### Commands

| Command | What it does |
| --- | --- |
| `/join` | Joins your voice channel and starts listening. `/join channel:#general` to pick one. |
| `/leave` | Leaves and releases the microphone. |
| `/voice` | Says which voice is in use and where to change it. |
| `/ping` | Checks the bot is alive. |

## Where your data lives

Two files, both next to the app:

- `butter-tts.settings.yaml` — your tokens, microphone, and tuning. **Plain text.** Anyone
  who can read the folder can read your keys, which is the trade for being portable and
  password-free. Keep it somewhere you trust, and revoke a key if it ever leaks.
- `butter-tts.transcripts.jsonl` — the text of everything you have said, most recent
  10,000 entries. **Text only; audio is never written to disk.**

What you say is sent to OpenAI to be transcribed and spoken. Nothing goes anywhere else,
and there is no telemetry.

## Building it yourself

Needs [Rust](https://rustup.rs) and [Node](https://nodejs.org) 22+. `songbird` compiles
libopus from C source, so **cmake and a C compiler must be on `PATH`** on every platform:

- **Windows**: Visual Studio Build Tools with the C++ workload gives you the compiler.
  cmake does not ship on `PATH` there, so install it separately
  (`winget install Kitware.CMake`).
- **macOS**: Xcode Command Line Tools (`xcode-select --install`) gives you the compiler.
  Install cmake with `brew install cmake`.

```sh
npm install
npm run tauri dev     # run it
npm run tauri build   # produces src-tauri/target/release/butter-tts(.exe)
```

Tests and lints:

```sh
cd src-tauri
cargo test --lib
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

On Windows the app renders through WebView2, which ships with Windows 11 — on an older
machine without it, Windows will offer to install the runtime. On macOS it renders through
the system WebKit, which every supported version already has.

## Releasing

Push a `v*` tag and [the workflow](.github/workflows/release.yml) builds Windows and
macOS binaries and attaches them to a GitHub release. The tag must match the version in
`src-tauri/tauri.conf.json`, or the build stops before it wastes time compiling.

```sh
git tag v0.2.0
git push origin v0.2.0
```

## How it fits together

```
mic ─▶ cpal ─▶ utterance detector ─▶ OpenAI transcribe ─▶ text
                                                            │
              Discord voice ◀─ songbird ◀─ OpenAI speak ◀────┘
```

The Rust side (`src-tauri/`) owns the bot, the audio, and the settings; the SvelteKit side
(`src/`) is the window. [AGENTS.md](AGENTS.md) has the coding standards for both.

## Licence

MIT
