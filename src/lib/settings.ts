// Mirrors the Settings struct in src-tauri/src/settings.rs. The field names are the YAML
// keys, so they stay snake_case rather than being renamed on the way across.

export type Settings = {
  openai_api_key: string;
  discord_bot_token: string;
  microphone_name: string;
  tts_voice: string;
  noise_suppression: boolean;
  speech_threshold: number;
  trailing_silence_ms: number;
  min_utterance_ms: number;
  max_utterance_ms: number;
};

// Where the database sits now, mirroring store::Placement in the backend: the OS's default
// folder, or some other folder the user chose.
export type DatabaseLocation = "default" | "custom";

export type SliderRange = {
  min: number;
  max: number;
  default: number;
};

// The bounds come from the backend rather than being repeated here, so the sliders and
// the clamping that guards the detector can never drift apart.
export type TuningRanges = {
  speechThreshold: SliderRange;
  trailingSilenceMs: SliderRange;
  minUtteranceMs: SliderRange;
  maxUtteranceMs: SliderRange;
};
