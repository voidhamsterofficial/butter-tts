# Butter TTS

A voice changer for Discord, for Windows and macOS.

You talk into your microphone. Butter TTS transcribes what you said and reads it back
into the voice channel in a synthetic voice. **Your real microphone audio never reaches
Discord** — the channel only ever hears the synthesised re-reading, and the recording of
your voice is thrown away as soon as it has been transcribed.

## Download

Grab the build for your platform from the [Releases page](https://github.com/voidhamsterofficial/butter-tts/releases):

- **Windows**: `butter-tts.exe` is a single portable file — no installer, no admin
  rights, nothing to uninstall. Prefer an installer instead? Grab the `-setup.exe`.
- **macOS**: the `.dmg` installs it like any other mac app — open it and drag
  **Butter TTS** into Applications.

> Windows may warn that the exe is unrecognised, because it is not code signed. Choose
> *More info* → *Run anyway*.
>
> macOS will refuse to open the app with a plain double-click, because it is not
> notarised. Right-click it and choose *Open* instead, then confirm in the dialog that
> appears — you only need to do this once.

## Setting it up

1. **Create a Discord bot** in the [developer portal](https://discord.com/developers/applications),
   add a bot to it, and copy the token. Invite it to your server with permission to join
   and speak in voice channels.
2. **Get an OpenAI API key** with some credit on it. It pays for the transcription and the
   speech.
3. **On first launch**, pick where to keep your settings and history — see
   [Where your data lives](#where-your-data-lives) below.
4. Open the app, go to **Settings**, paste both in, choose your microphone, and save.
5. Press **Wake up** on the Home page.
6. In Discord, sit in a voice channel and type `/join`. Then talk.

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

Everything — your OpenAI key, Discord token, microphone choice, tuning, and the text of
everything you have said — lives in one SQLite file, `butter-tts.db`. The two credential
fields are encrypted in it; see [Your keys and your privacy](#your-keys-and-your-privacy).

The first time the app runs, it asks where to put that file:

- **Default** (recommended): your system's own app data folder. Survives the app being
  updated or reinstalled.
- **Portable**: right next to the app itself, so the whole folder works the same from a
  USB stick. This is the natural choice for the portable Windows exe. On an installed
  app (the Windows installer, or the macOS `.dmg`), the folder it sits in gets replaced
  on every update or reinstall, so pick this only if you are fine with that trade.

Whichever you choose, the Settings and History pages both show exactly where the file
ended up, with a button to open its folder.

## Your keys and your privacy

Your OpenAI key and Discord token are **encrypted** in the database. That guards against
casual exposure — opening the file in a text or hex editor, a cloud-sync provider's
content scanner, a screenshot of a file browser — but it is not a password: there is
nothing to type on launch and nothing hidden in the OS's credential store, so anyone who
can open the database with this app installed can still use your bot and spend against
your OpenAI account. That trade is what keeps the app password-free. If a key ever
leaks, revoke it: regenerate the bot token in the Discord developer portal and the key in
your OpenAI account.

History keeps the most recent 10,000 things you have said, oldest dropped first. **Text
only — the audio of your voice is never written to disk.**

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
npm run tauri build   # produces the portable src-tauri/target/release/butter-tts(.exe)
```

To also produce an installer locally:

```sh
npm run tauri build -- --bundles nsis   # Windows: src-tauri/target/release/bundle/nsis
npm run tauri build -- --bundles dmg    # macOS: src-tauri/target/release/bundle/dmg
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

Push a `v*` tag and [the workflow](.github/workflows/release.yml) builds the Windows exe,
the Windows installer, and the macOS installer, and attaches all three to a GitHub
release. The tag must match the version in `src-tauri/tauri.conf.json`, or the build
stops before it wastes time compiling.

```sh
git tag v0.4.0
git push origin v0.4.0
```

## How it fits together

```
mic ─▶ cpal ─▶ utterance detector ─▶ OpenAI transcribe ─▶ text
                                                            │
              Discord voice ◀─ songbird ◀─ OpenAI speak ◀────┘
```

The Rust side (`src-tauri/`) owns the bot, the audio, and the database (`store/`); the
SvelteKit side (`src/`) is the window. [AGENTS.md](AGENTS.md) has the coding standards
for both.

## Licence

MIT
