// The app's live view of the bot: its status, its log, and the microphone level.
// One store for the whole window — the sidebar, the dashboard and the console are all
// looking at the same bot.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type BotState = "offline" | "starting" | "online" | "reconnecting" | "failed";

export type BotStatus =
  | { state: "offline" }
  | { state: "starting" }
  | { state: "online" }
  | { state: "reconnecting" }
  | { state: "failed"; detail: string };

export type LogLine = {
  timestampMs: number;
  level: string;
  message: string;
};

export type Transcript = {
  timestamp_ms: number;
  text: string;
  voice: string;
};

export type VoiceChannel = {
  id: string;
  name: string;
};

export type GuildVoiceChannels = {
  guildId: string;
  guildName: string;
  channels: VoiceChannel[];
};

// A long session would otherwise grow the log forever. Old lines are the ones to drop:
// what just happened is what anyone opening the console is looking for.
const MAX_LOG_LINES = 1000;

// The backend sends the raw peak of every ~10ms chunk, about 100 times a second. Shown
// as-is that reading is violently spiky — one loud sample jerks the whole meter, and the
// natural gaps between syllables make it strobe. Everything below turns that raw stream
// into something that reads smoothly and holds steady.

// The displayed level follows the raw one as an exponential moving average: it rises
// quickly so the meter reacts the instant you talk, and falls slowly so it glides back
// down through the gaps in speech instead of flickering. Both are per-chunk fractions.
const LEVEL_ATTACK = 0.45;
const LEVEL_RELEASE = 0.06;

// "Speaking" needs to survive the pauses in normal speech. It takes a clear reading to
// switch on, a quieter one to switch off, and even then only after a short hold — so a
// breath between words never drops it. This hysteresis is what makes it feel consistent.
const SPEAKING_ON_LEVEL = 0.09;
const SPEAKING_OFF_LEVEL = 0.045;
const SPEAKING_HOLD_MS = 550;

// The tuning meter shows the loudest recent moment rather than a smoothed average,
// because the detector triggers on any chunk that crosses the threshold — a peak-hold
// that eases down matches what actually trips it, and is still readable.
const PEAK_DECAY = 0.9;

// The meter decays to zero on its own, so a stalled level event (the session ended)
// leaves it empty rather than frozen mid-reading.
const LEVEL_IDLE_TIMEOUT_MS = 400;

class BotStore {
  status = $state<BotStatus>({ state: "offline" });
  logLines = $state<LogLine[]>([]);
  transcripts = $state<Transcript[]>([]);
  inputLevel = $state(0);
  isBusy = $state(false);

  // Connected to Discord is not the same as listening. Nothing is captured until the bot
  // is in a voice channel, so this is what "Listening" should actually key off.
  inChannel = $state(false);

  /** The loudest the mic has been recently, 0-32767, for the tuning meter. */
  peakAmplitude = $state(0);

  #levelTimer: ReturnType<typeof setTimeout> | null = null;
  #speaking = $state(false);
  // When the level last sat above the "off" line, so the hold can be measured from it.
  #lastLoudAt = 0;

  get isSpeaking(): boolean {
    return this.inChannel && this.#speaking;
  }

  /** Starts listening for everything the backend reports. Returns a teardown function. */
  async connect(): Promise<UnlistenFn> {
    const unlisteners = await Promise.all([
      listen<BotStatus>("bot://status", (event) => {
        this.status = event.payload;

        // Leaving "online" means the bot is no longer in any channel, whatever the last
        // session event was — a crash or stop may not send one.
        if (event.payload.state !== "online") {
          this.inChannel = false;
        }
      }),
      listen<boolean>("bot://session", (event) => {
        this.inChannel = event.payload;
      }),
      listen<LogLine>("bot://log", (event) => {
        this.#appendLogLine(event.payload);
      }),
      listen<number>("bot://level", (event) => {
        this.#setLevel(event.payload);
      }),
      listen<Transcript>("bot://transcript", (event) => {
        this.transcripts = [...this.transcripts, event.payload];
      }),
    ]);

    // The bot may already be running if the window reloaded, so ask rather than assume.
    this.status = await invoke<BotStatus>("bot_status");
    this.transcripts = await invoke<Transcript[]>("load_transcripts");

    return () => {
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }

  #appendLogLine(line: LogLine) {
    const lines = [...this.logLines, line];
    this.logLines = lines.slice(-MAX_LOG_LINES);
  }

  #setLevel(level: number) {
    // Glide toward the new reading: fast when it is rising, slow when it is falling.
    const smoothing = level > this.inputLevel ? LEVEL_ATTACK : LEVEL_RELEASE;
    this.inputLevel += (level - this.inputLevel) * smoothing;

    // Peak-hold in raw amplitude for the tuning meter: snap up to any louder moment,
    // ease down from a quieter one. That is the number the threshold is measured against.
    const rawPeak = Math.round(level * 32767);
    this.peakAmplitude =
      rawPeak > this.peakAmplitude
        ? rawPeak
        : Math.round(this.peakAmplitude * PEAK_DECAY);

    this.#updateSpeaking();

    if (this.#levelTimer !== null) {
      clearTimeout(this.#levelTimer);
    }

    this.#levelTimer = setTimeout(() => {
      this.inputLevel = 0;
      this.peakAmplitude = 0;
      this.#speaking = false;
    }, LEVEL_IDLE_TIMEOUT_MS);
  }

  /** Switches "speaking" on and off with hysteresis and a hold, so it never flickers. */
  #updateSpeaking() {
    const now = Date.now();

    if (this.inputLevel >= SPEAKING_ON_LEVEL) {
      this.#speaking = true;
      this.#lastLoudAt = now;
      return;
    }

    if (this.inputLevel >= SPEAKING_OFF_LEVEL) {
      // In the dead band between the two lines: hold whatever it already was, and treat
      // this as still-loud so the hold timer keeps resetting through quiet speech.
      this.#lastLoudAt = now;
      return;
    }

    // Clearly quiet now. Only drop once it has stayed quiet for the hold, so a pause for
    // breath does not read as the end of talking.
    if (now - this.#lastLoudAt >= SPEAKING_HOLD_MS) {
      this.#speaking = false;
    }
  }

  clearLog() {
    this.logLines = [];
  }

  /** Copies the whole console log to the clipboard as plain text. Returns whether it
   *  landed, so a caller can show feedback. Shared by the console page's button and the
   *  right-click menu, so the two produce identical text. */
  async copyLog(): Promise<boolean> {
    const text = this.logLines
      .map((line) => `${formatTime(line.timestampMs)} ${line.level.toUpperCase()} ${line.message}`)
      .join("\n");

    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  }

  async clearTranscripts(): Promise<string | null> {
    try {
      await invoke("clear_transcripts");
      this.transcripts = [];
      return null;
    } catch (error) {
      return String(error);
    }
  }

  async start(): Promise<string | null> {
    this.isBusy = true;

    try {
      await invoke("start_bot");
      return null;
    } catch (error) {
      // The backend already set the status to failed and logged the reason; this is for
      // showing it next to the button that was just pressed.
      return String(error);
    } finally {
      this.isBusy = false;
    }
  }

  async stop(): Promise<string | null> {
    this.isBusy = true;

    try {
      await invoke("stop_bot");
      return null;
    } catch (error) {
      return String(error);
    } finally {
      this.isBusy = false;
    }
  }

  /** Every server and voice channel the bot can see, for the app's own channel picker —
   *  there is no Discord command that does this. */
  async listVoiceChannels(): Promise<GuildVoiceChannels[]> {
    return invoke<GuildVoiceChannels[]>("list_voice_channels");
  }

  async joinChannel(guildId: string, channelId: string): Promise<string | null> {
    this.isBusy = true;

    try {
      await invoke("join_voice_channel", { guildId, channelId });
      return null;
    } catch (error) {
      return String(error);
    } finally {
      this.isBusy = false;
    }
  }

  async leaveChannel(): Promise<string | null> {
    this.isBusy = true;

    try {
      await invoke("leave_voice_channel");
      return null;
    } catch (error) {
      return String(error);
    } finally {
      this.isBusy = false;
    }
  }

  /** Whether a bot is running or on its way there, so pressing the button stops it
   *  rather than trying to start a second one. Covers the in-between states — a
   *  "starting" that has not finished connecting, and a "reconnecting" that dropped —
   *  which are still a live bot the backend would refuse to start again. */
  get isRunning(): boolean {
    return (
      this.status.state === "online" ||
      this.status.state === "starting" ||
      this.status.state === "reconnecting"
    );
  }

  async toggle(): Promise<string | null> {
    return this.isRunning ? this.stop() : this.start();
  }
}

export const bot = new BotStore();

/** The words next to the status dot, for whatever the bot is currently doing. */
export function describeStatus(status: BotStatus, inChannel: boolean, isSpeaking: boolean): string {
  switch (status.state) {
    case "offline":
      return "Offline";
    case "starting":
      return "Connecting…";
    case "reconnecting":
      return "Reconnecting…";
    case "failed":
      return "Failed to start";
    case "online":
      // Connected but sitting in no channel: awake, not yet hearing anything.
      if (!inChannel) {
        return "Ready";
      }
      return isSpeaking ? "Hearing you" : "Listening";
  }
}

/** Which colour the status should read as. */
export function statusTone(status: BotStatus, inChannel: boolean, isSpeaking: boolean): string {
  if (status.state === "online") {
    if (!inChannel) {
      return "ready";
    }
    return isSpeaking ? "speaking" : "online";
  }

  return status.state;
}

export function formatTime(timestampMs: number): string {
  const time = new Date(timestampMs);
  const hours = String(time.getHours()).padStart(2, "0");
  const minutes = String(time.getMinutes()).padStart(2, "0");
  const seconds = String(time.getSeconds()).padStart(2, "0");

  return `${hours}:${minutes}:${seconds}`;
}
